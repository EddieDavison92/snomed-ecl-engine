use snomed_ecl_engine::store::Manifest;
use std::io::{self, IsTerminal};

pub fn human() -> bool {
    io::stdout().is_terminal()
}

pub fn clean(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

/// Escapes control characters as `clean` does, but keeps line breaks, so a
/// message built over several lines still reads as several lines.
pub fn clean_message(text: &str) -> String {
    text.split('\n').map(clean).collect::<Vec<_>>().join("\n")
}

pub fn heading(text: &str) -> String {
    if human()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").as_deref() != Ok("dumb")
    {
        format!("\x1b[1;36m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

pub fn bytes(value: u64) -> String {
    match value {
        0..1_048_576 => format!("{:.0} KiB", value as f64 / 1024.0),
        1_048_576..1_073_741_824 => format!("{:.0} MiB", value as f64 / 1_048_576.0),
        _ => format!("{:.2} GiB", value as f64 / 1_073_741_824.0),
    }
}

/// Points at the byte offset an ECL parse error reported. Long expressions are
/// windowed so the caret stays beside the offending text on one terminal line.
pub fn caret(expression: &str, offset: usize) -> String {
    let offset = offset.min(expression.len());
    let line_start = expression[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line_end = expression[offset..]
        .find('\n')
        .map_or(expression.len(), |i| offset + i);
    let (mut start, mut end) = (line_start, line_end);
    let mut prefix = "";
    let mut suffix = "";
    if end - start > 72 {
        start = start.max(offset.saturating_sub(36));
        while !expression.is_char_boundary(start) {
            start -= 1;
        }
        end = end.min(start + 72);
        while !expression.is_char_boundary(end) {
            end -= 1;
        }
        prefix = if start > line_start { "..." } else { "" };
        suffix = if end < line_end { "..." } else { "" };
    }
    let text = clean(&expression[start..end]);
    let column = clean(&expression[start..offset]).chars().count() + prefix.len();
    format!("  {prefix}{text}{suffix}\n  {}^", " ".repeat(column))
}

pub fn number(value: usize) -> String {
    let digits = value.to_string();
    let mut result = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

pub fn manifest(m: &Manifest, location: Option<&str>) {
    println!("{}\n", heading("SNOMED ECL / index"));
    if let Some(location) = location {
        println!("  Index       {}", clean(location));
    }
    println!("  Edition     {}", clean(&m.edition));
    println!("  Format      {}", m.format);
    println!(
        "  Concepts    {} active / {} total",
        number(m.active_concept_count),
        number(m.concept_count)
    );
    println!("  Hierarchy   {} edges", number(m.hierarchy_edges));
    println!(
        "  Attributes  {} ordinary / {} concrete",
        number(m.attributes),
        number(m.concrete_attributes)
    );
    println!("  Core        {:.2} MiB", m.core_bytes as f64 / 1_048_576.0);
    if let Some(membership) = &m.membership {
        println!(
            "  Membership  {:.2} MiB / {} refsets / {} concept pairs",
            membership.bytes as f64 / 1_048_576.0,
            number(membership.refset_count),
            number(membership.concept_pairs)
        );
    } else {
        println!("  Membership  Not built; reimport RF2 to enable refset queries");
    }
    println!(
        "  Displays    {:.2} MiB (separate file)",
        m.display_bytes as f64 / 1_048_576.0
    );
    if let Some(descriptions) = &m.descriptions {
        println!(
            "  Descriptions {:.2} MiB / {} terms / {} language memberships (loaded on demand)",
            descriptions.bytes as f64 / 1_048_576.0,
            number(descriptions.descriptions),
            number(descriptions.language_memberships)
        );
    }
    for supplement in &m.supplements {
        println!(
            "  Supplement  {} / {} refsets / {} new concepts",
            supplement.release_date,
            number(supplement.refset_ids.len()),
            number(supplement.added_concepts)
        );
    }
    if let Some(tables) = &m.member_tables {
        println!(
            "  Member fields {:.2} MiB / {} refsets (loaded on demand)",
            tables.iter().map(|t| t.bytes).sum::<u64>() as f64 / 1_048_576.0,
            number(tables.len())
        );
    }
    println!("\n  Manifest metadata only; each index file is verified when loaded.");
    println!("  ECL 2.3 support and open questions: docs/ecl-support.md.");
}

/// Prints discovered indexes, marking the selected one. Editions are shown as
/// the version segment alone; `stats` gives the full URI.
pub fn stores(found: &[crate::workspace::Found]) {
    println!("{}\n", heading("SNOMED ECL / indexes"));
    let home = crate::library::home()
        .map(|home| home.display().to_string())
        .unwrap_or_else(|_| "not available".into());
    if found.is_empty() {
        println!("  No indexes yet. Build one from an RF2 release:\n");
        println!("    snomed-ecl-engine add ARCHIVE.zip\n");
        println!("  The library folder is {}.", clean(&home));
        return;
    }
    // Library indexes by name; others by path.
    let labels: Vec<String> = found
        .iter()
        .map(|entry| match &entry.name {
            Some(name) => clean(name),
            None => clean(&entry.path.display().to_string()),
        })
        .collect();
    let width = labels
        .iter()
        .map(|label| label.chars().count())
        .max()
        .unwrap_or(5)
        .clamp(5, 48);
    println!(
        "    {:<width$}  {:<16}  {:>9}  {:>8}",
        "INDEX", "RELEASE", "CONCEPTS", "SIZE"
    );
    println!("  {}", "-".repeat(width + 43));
    for (entry, label) in found.iter().zip(&labels) {
        let release = match crate::library::edition_parts(&entry.edition) {
            Some((family, date)) => format!("{family} {}", crate::library::show_date(&date)),
            None => clean(&entry.edition),
        };
        println!(
            "  {} {:<width$}  {:<16}  {:>9}  {:>8}{}",
            if entry.selected { "*" } else { " " },
            label,
            release,
            number(entry.active_concepts),
            bytes(entry.bytes),
            if entry.packed { "" } else { "  directory" },
        );
    }
    println!("\n  * selected. Concepts are active concepts; `stats` has the full manifest.");
    println!("  Select one with `use NAME`, or `use uk` for the latest UK release.");
    println!("  Library folder: {}", clean(&home));
}

/// The command list, with the selected index and the next useful step. Shown
/// bare or with `help`, so it is the first thing most users read.
fn root() {
    println!("Evaluate SNOMED CT ECL against a local index built from an RF2 release.\n");
    println!("Usage: snomed-ecl-engine COMMAND [ARGS] [--json|--plain]\n");
    println!("Indexes:");
    println!("  download    Fetch a release from NHS England TRUD and add it");
    println!("  add         Build an index from an RF2 archive and select it");
    println!("  list        List your indexes and their releases");
    println!("  use         Select an index by name, such as uk or uk@2026-08");
    println!("  remove      Delete an index from the library");
    println!("  stats       Show an index's edition, counts and size");
    println!("  verify      Check every section of an index\n");
    println!("Building by hand:");
    println!("  inspect     Read an archive's release metadata");
    println!("  import      Build an index directory from an RF2 archive");
    println!("  add-refsets Add a simple RF2 refset supplement to an index");
    println!("  pack        Pack an index directory into one compressed file\n");
    println!("Query:");
    println!("  query       Evaluate ECL expressions interactively");
    println!("  expand      Evaluate one ECL expression");
    println!("  search      Find concepts by name");
    println!("  lookup      Describe one concept: terms, parents, attributes, refsets");
    println!("  history     Show what replaced a concept, and what it replaced");
    println!("  hierarchy   List a concept's parents, children, ancestors or descendants");
    println!("  diff        Compare one expression across two indexes");
    println!("  batch       Answer JSONL requests on stdin, for scripts and agents\n");
    println!("Run COMMAND --help for arguments and examples.\n");
    match crate::workspace::load().store {
        Some(store) => {
            println!("Selected index: {}", clean(&store.display().to_string()));
            println!(
                "Commands taking [STORE] use it unless given a name, a path or SNOMED_ECL_STORE."
            );
            println!("\nNext: snomed-ecl-engine query");
        }
        None => {
            println!("No index selected.");
            println!("\nNext: snomed-ecl-engine add ARCHIVE.zip, or `list` to see your indexes.");
        }
    }
    println!("\nOutput:");
    println!("  Terminal   Readable summaries and display tables");
    println!("  Redirected Plain code lines or JSON output for scripts");
    println!("  --json     Explicit JSON output (JSONL for expansion rows)");
    println!("  --plain    Plain script output, even in a terminal");
    println!("  NO_COLOR   Disable colour");
    println!(
        "\nEvaluates ECL 2.3. Queries it cannot answer fail explicitly; see docs/ecl-support.md."
    );
    println!("Agent setup and query workflow: SKILL.md");
}

pub fn help(command: Option<&str>) -> anyhow::Result<()> {
    println!("{}  {}\n", heading("SNOMED ECL"), env!("CARGO_PKG_VERSION"));
    match command {
        Some("pack") => println!("Usage: pack STORE DESTINATION_FILE [--uncompressed|--block-kib 16|64]\n\nPack a directory index or repack an existing container into one file.\nAll sections and semantic data are preserved. Default: zstd level 15, 64 KiB blocks.\n--uncompressed provides a comparison baseline. No RF2 reimport is needed.\nDESTINATION_FILE must not exist. Packing uses temporary disk space beside it.\nPublication requires hard-link support on that filesystem.\nUse the resulting file with stats, verify, expand, batch or add-refsets.\nCompression reduces file size; decoded query indexes still occupy memory."),
        Some("verify") => println!("Usage: verify [STORE]\n\nCheck every component checksum and its structure, including cold sections.\nSTORE may be a directory or a packed index file.\nDescription and member sections are checked individually.\nOrdinary queries continue to verify each section when it is first loaded."),
        None | Some("help") => root(),
        Some("import") => println!("Usage: import ARCHIVE DESTINATION EDITION_URI SHA256 [DISPLAY_REFSET_IDS]\n\nRequires one self-contained Snapshot ZIP, a versioned edition URI and\na trusted archive SHA-256. DESTINATION must not exist.\nOptional display refsets are comma-separated IDs in preference order.\nThe default is NHS clinical realm, pharmacy realm, then GB English.\n\nStages and elapsed time go to stderr. Redirected stdout contains JSON.\nThis build {} import support.", if cfg!(feature = "import") { "includes" } else { "excludes" }),
        Some("add-refsets") => println!("Usage: add-refsets BASE_STORE ARCHIVE DESTINATION RELEASE_DATE SHA256\n\nLoad simple concept refsets from a verified RF2 Snapshot ZIP.\nInclude new defining concepts and inferred is-a relationships when supplied.\nRELEASE_DATE is YYYYMMDD. DESTINATION must not exist.\nExisting definitions and populated refsets cannot be replaced.\nFor an updated supplement, start from the original base store.\nDescriptions and typed simple members are preserved when the base has those indexes. Arbitrary maps are not imported.\nThe base display index is required. See docs/indexes.md for scope and provenance."),
        Some("use") => println!("Usage: use [NAME | PATH | --clear]\n\nRemember one index, so later commands need no path. NAME is a library\nindex such as uk-20260826, a release such as uk@2026-08, or an edition\nalone, such as uk, for its latest release. With no argument, shows the\nselection. SNOMED_ECL_STORE overrides it for one shell."),
        Some("list" | "stores") => println!("Usage: list [FOLDER...]\n\nList the library's indexes by name and release, then any other index\nfound directly inside . and data, or inside the folders given. The\nlibrary folder is set by SNOMED_ECL_HOME, or defaults to the platform's\ndata folder. `stores` is another name for this command."),
        Some("add") => println!("Usage: add ARCHIVE [--sha256 HEX] [--name NAME] [--edition URI]\n\nBuild an index from an RF2 Snapshot ZIP, pack it into the library and\nselect it. The name defaults to the edition and release date, such as\nuk-20260826.\n\n  --sha256   The checksum your distributor published. Without it, the\n             archive's checksum is shown and you are asked to confirm it.\n  --name     Another name for the index.\n  --edition  The edition URI, when the archive does not name one.\n\nExample: snomed-ecl-engine add uk_sct2mo_42.5.0_20260826000001Z.zip --sha256 1330d2f2..."),
        Some("download") => println!("Usage: download [ITEM] [--list | --release ID] [--name NAME] [--keep-archive]\n\nFetch a release from NHS England's TRUD service, check it against the\nSHA-256 TRUD publishes, then build, pack and select it as `add` does.\nITEM is uk-monolith (the default) or a TRUD item number.\n\n  --list          List the item's releases instead\n  --release ID    Fetch this release rather than the newest\n  --name NAME     Another name for the index\n  --keep-archive  Keep the ZIP in the library's downloads folder\n\nSet TRUD_API_KEY to your TRUD API key first. Your TRUD account must be\nsubscribed to the item. The key is never printed."),
        Some("remove") => println!("Usage: remove NAME [--yes]\n\nDelete one index from the library, after asking. NAME is a library name,\na release such as uk@2026-08, or an edition such as uk; paths are not\naccepted, so nothing outside the library is deleted. --yes skips the\nquestion, for scripts."),
        Some("query") => println!("Usage: query [STORE] [--display|--count] [--config FILE]\n\nOpen one index and evaluate ECL expressions until :quit.\nThe index is opened and verified once, so later expressions answer immediately.\n  :display  toggle result terms\n  :count    toggle totals only\n  :stats    show the index manifest\nParse and evaluation errors return to the prompt and do not end the session.\nResults are listed in pages; the total is always reported in full.\nUse expand for one expression, or batch for scripted JSONL queries."),
        Some("diff") => println!("Usage: diff OLD_STORE NEW_STORE ECL [--display|--count] [--config FILE]\n\nEvaluate one expression against two indexes and report added and removed codes.\nUse it to see what a release or refset version changed for a definition.\nBoth indexes are opened in turn, not together.\nTerms are resolved from the index each code belongs to, so removed concepts\nstill get the term the older index held.\nRedirected output, or --json, gives the complete added and removed code sets.\nConcept results only; member projections returning values or rows are refused."),
        Some("inspect") => println!("Usage: inspect ARCHIVE

Read an RF2 archive's release metadata without importing it, and print the
import command for it. Reports the SHA-256, the release date, which required
Snapshot files are present, and the edition URIs the archive's own module
dependencies declare.

The checksum proves the file is intact, not where it came from. Compare it
with the value the release distributor published before importing."),
        Some("stats") => println!("Usage: stats [STORE] [--json|--plain]\n\nRead manifest metadata without loading the numeric index.\nExample: snomed-ecl-engine stats --json"),
        Some("expand") => println!("Usage: expand [STORE] ECL [--display|--codes|--count|--csv] [--json|--plain] [--config FILE]\n\nQuote ECL so the shell preserves operators, spaces and SNOMED terms.\n  --count    Return only the total\n  --display  Add each concept's term; the default in a terminal\n  --codes    Codes only, even in a terminal\n  --csv      A code,display table of every concept, for spreadsheets\n  --json     Emit code objects as JSONL, or a total object with --count\n\nExample: snomed-ecl-engine expand uk \"<< 404684003\" --csv > findings.csv\n\nA terminal lists the first 200 with the total; redirected output, --json\nand --csv return every concept. Member projections can return typed JSON\nrows; --count counts those rows, and --display and --csv need concepts.\nUse --config FILE for identifier-scheme and dialect aliases (docs/indexes.md).\nFor repeated queries, use batch to open the index once."),
        Some("search") => println!("Usage: search [STORE] TEXT [--within ECL] [--limit N] [--inactive]\n\nFind concepts whose terms contain every word, best match first. Words\nmatch from their start, so \"chron kid\" finds chronic kidney disease.\n  --within ECL  Search only the concepts an expression selects\n  --limit N     Show N matches (default 50)\n  --inactive    Include inactive concepts\n\nExample: snomed-ecl-engine search heart failure --within \"<< 404684003\""),
        Some("lookup" | "concept") => println!("Usage: lookup [STORE] SCTID\n\nDescribe one concept: its terms, status, parents, children, attribute\ngroups and reference set membership. `concept` is another name for it."),
        Some("history") => println!("Usage: history [STORE] SCTID\n\nShow a concept's historical associations: what replaced it, and what it\nreplaced, with the association for each."),
        Some("batch") => println!("Usage: batch [STORE] [--config FILE]\n\nRead one JSON object per line from stdin:\n  {{\"ecl\":\"<< 404684003\",\"count_only\":true}}\n\nEach response includes edition, query_config_sha256, total, parse_ms and eval_ms.\nOmit count_only or set it false to include codes as decimal strings.\nQuery errors return an error object; later queries still run.\nEnd stdin to exit. Output stays JSONL in terminals too."),
        Some("hierarchy") => println!("Usage: hierarchy [STORE] OPERATOR SCTID [--display]\n\nOperators: <  <<  <!  <<!  >  >>  >!  >>!\nQuote the operator. Output is code lines, or JSONL with --display.\nUse expand for full expressions and terminal display tables."),
        Some(_) => anyhow::bail!("Unknown command. Run --help for available commands"),
    }
    Ok(())
}

/// A JSON string field, cleaned for the terminal, or a stand-in when absent.
fn text(value: &serde_json::Value, key: &str) -> String {
    value[key]
        .as_str()
        .map_or_else(|| "(no display)".into(), clean)
}

/// A concept as `CODE  label`, marking an inactive one.
fn concept_line(value: &serde_json::Value) -> String {
    let inactive = value["active"].as_bool() == Some(false);
    format!(
        "{:<18}  {}{}",
        value["code"].as_str().unwrap_or(""),
        text(value, "display"),
        if inactive { "  (inactive)" } else { "" }
    )
}

/// Lists concepts under a heading, at most `limit` of them.
fn concept_list(title: &str, concepts: &[serde_json::Value], limit: usize) {
    println!("\n  {title} ({})", number(concepts.len()));
    for concept in concepts.iter().take(limit) {
        println!("    {}", concept_line(concept));
    }
    if concepts.len() > limit {
        println!("    ... {} more", number(concepts.len() - limit));
    }
}

pub fn search(value: &serde_json::Value, query: &str) {
    let concepts = value["concepts"].as_array().cloned().unwrap_or_default();
    let total = value["total"].as_u64().unwrap_or(0) as usize;
    println!("{}\n", heading("SNOMED ECL / search"));
    println!(
        "  \"{}\": {} matching concepts, best {} shown\n",
        clean(query),
        number(total),
        number(concepts.len())
    );
    for concept in &concepts {
        println!("  {}", concept_line(concept));
    }
    if concepts.is_empty() {
        println!("  Nothing matches every word. Try fewer or shorter words.");
    } else {
        println!("\n  `lookup CODE` describes one; --limit N shows more.");
    }
}

pub fn concept(value: &serde_json::Value) {
    println!("{}\n", heading("SNOMED ECL / concept"));
    println!(
        "  {}  {}",
        value["code"].as_str().unwrap_or(""),
        text(value, "display")
    );
    if let Some(fsn) = value["fsn"].as_str() {
        println!("  {:<18}  {}", "", clean(fsn));
    }
    let status = format!(
        "{}, {}",
        if value["active"].as_bool() == Some(true) {
            "active"
        } else {
            "inactive"
        },
        if value["defined"].as_bool() == Some(true) {
            "fully defined"
        } else {
            "primitive"
        }
    );
    println!("\n  Status    {status}");
    println!("  Module    {}", text(&value["module"], "display"));
    let empty = Vec::new();
    let list = |key: &str| value[key].as_array().unwrap_or(&empty).clone();
    concept_list("Parents", &list("parents"), 12);
    concept_list("Children", &list("children"), 12);
    let groups = list("groups");
    if !groups.is_empty() {
        println!("\n  Attributes");
        for group in &groups {
            let number = group["group"].as_u64().unwrap_or(0);
            let label = if number == 0 {
                "ungrouped".to_owned()
            } else {
                format!("group {number}")
            };
            println!("    {label}");
            for attribute in group["attributes"].as_array().unwrap_or(&empty) {
                let target = if attribute["target"].is_object() {
                    format!(
                        "{} ({})",
                        text(&attribute["target"], "display"),
                        attribute["target"]["code"].as_str().unwrap_or("")
                    )
                } else {
                    // ECL syntax: #number, "string", or true and false.
                    let literal = &attribute["value"]["value"];
                    match (attribute["value"]["type"].as_str(), literal.as_str()) {
                        (Some("number"), Some(number)) => format!("#{number}"),
                        (Some("text"), Some(text)) => format!("\"{}\"", clean(text)),
                        _ => literal.to_string(),
                    }
                };
                println!(
                    "      {} = {}",
                    text(&attribute["type"], "display"),
                    target.trim_end()
                );
            }
        }
    }
    let synonyms: Vec<_> = list("descriptions")
        .into_iter()
        .filter(|d| d["active"].as_bool() == Some(true))
        .collect();
    if !synonyms.is_empty() {
        println!("\n  Descriptions ({})", number(synonyms.len()));
        for description in synonyms.iter().take(12) {
            let preferred = description["preferred_in"]
                .as_array()
                .is_some_and(|p| !p.is_empty());
            println!(
                "    {} {}",
                if preferred { "*" } else { " " },
                clean(description["term"].as_str().unwrap_or(""))
            );
        }
        if synonyms.len() > 12 {
            println!("      ... {} more", number(synonyms.len() - 12));
        }
        println!("    * preferred in at least one language reference set");
    }
    concept_list("Reference sets", &list("refsets"), 8);
}

pub fn history(value: &serde_json::Value) {
    println!("{}\n", heading("SNOMED ECL / history"));
    println!(
        "  {}  {}",
        value["concept"].as_str().unwrap_or(""),
        if value["active"].as_bool() == Some(true) {
            "active"
        } else {
            "inactive"
        }
    );
    for (title, key) in [("Replaced by", "successors"), ("Replaces", "predecessors")] {
        let rows = value[key].as_array().cloned().unwrap_or_default();
        println!("\n  {title} ({})", number(rows.len()));
        for row in rows.iter().take(20) {
            println!(
                "    {}  [{}]",
                concept_line(&row["concept"]),
                text(&row["association"], "display")
            );
        }
        if rows.len() > 20 {
            println!("    ... {} more", number(rows.len() - 20));
        }
    }
}

/// Describes a result's size with the unit its projection returned.
pub fn total(result: &snomed_ecl_engine::eval::QueryResult) -> String {
    use snomed_ecl_engine::eval::QueryResult;
    let unit = match result {
        QueryResult::Concepts(_) => "concepts",
        QueryResult::Values(_) => "values",
        QueryResult::Rows(_) => "rows",
    };
    format!("{} {unit}", number(result.len()))
}
