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

pub fn manifest(m: &Manifest) {
    println!("{}\n", heading("SNOMED ECL / index"));
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
    println!("\n  Manifest metadata only; each index file is verified when loaded.");
    println!("  Full ECL 2.3 is in development. See docs/conformance.md.");
}

pub fn help(command: Option<&str>) -> anyhow::Result<()> {
    println!("{}  {}\n", heading("SNOMED ECL"), env!("CARGO_PKG_VERSION"));
    match command {
        None | Some("help") => println!("Import a verified RF2 Snapshot and query its local index.\n\nUsage: snomed-ecl-engine COMMAND [ARGS] [--json|--plain]\n\nCommands:\n  import     Build an immutable index from an RF2 archive\n  add-refsets Add a simple RF2 refset supplement to an existing index\n  stats      Inspect edition, counts and index size\n  expand     Evaluate one ECL expression\n  batch      Reuse one index for JSONL queries on stdin\n  hierarchy  Traverse an is-a hierarchy directly\n\nRun COMMAND --help for arguments and examples.\n\nOutput:\n  Terminal   Readable summaries and display tables\n  Redirected Original code lines or JSON output for scripts\n  --json     Explicit JSON output (JSONL for expansion rows)\n  --plain    Original script output, even in a terminal\n  NO_COLOR   Disable colour\n\nFull ECL 2.3 is in development. Unsupported queries fail explicitly.\nAgent setup and query workflow: SKILL.md"),
        Some("import") => println!("Usage: import ARCHIVE DESTINATION EDITION_URI SHA256 [DISPLAY_REFSET_IDS]\n\nRequires one self-contained Snapshot ZIP, a versioned edition URI and\na trusted archive SHA-256. DESTINATION must not exist.\nOptional display refsets are comma-separated IDs in preference order.\nThe default is NHS clinical realm, pharmacy realm, then GB English.\n\nStages and elapsed time go to stderr. Redirected stdout contains JSON.\nThis build {} import support.", if cfg!(feature = "import") { "includes" } else { "excludes" }),
        Some("add-refsets") => println!("Usage: add-refsets BASE_STORE ARCHIVE DESTINATION RELEASE_DATE SHA256\n\nLoad simple concept refsets from a verified RF2 Snapshot ZIP.\nInclude new defining concepts and inferred is-a relationships when supplied.\nRELEASE_DATE is YYYYMMDD. DESTINATION must not exist.\nExisting definitions and populated refsets cannot be replaced.\nFor an updated supplement, start from the original base store.\nDescriptions and language memberships are preserved when the base has a description index. Other typed refset fields and maps are not imported.\nThe base display index is required. See docs/refsets.md for scope and provenance."),
        Some("stats") => println!("Usage: stats STORE [--json|--plain]\n\nRead manifest metadata without loading the numeric index.\nExample: snomed-ecl-engine stats data/compact-store/v1 --json"),
        Some("expand") => println!("Usage: expand STORE ECL [--display|--count] [--json|--plain]\n\nQuote ECL so the shell preserves operators, spaces and SNOMED terms.\n  --count    Return only the total\n  --display  Resolve labels after evaluating the complete result set\n  --json     Emit code objects as JSONL, or a total object with --count\n\nExample: snomed-ecl-engine expand data/compact-store/v1 \"<< 404684003\" --count\n\nAll matches are returned; there is no implicit result limit.\nFor repeated queries, use batch to open the index once."),
        Some("batch") => println!("Usage: batch STORE\n\nRead one JSON object per line from stdin:\n  {{\"ecl\":\"<< 404684003\",\"count_only\":true}}\n\nEach response is one JSON line with edition, total, parse_ms and eval_ms.\nOmit count_only or set it false to include codes as decimal strings.\nQuery errors return an error object; later queries still run.\nEnd stdin to exit. Output stays JSONL in terminals too."),
        Some("hierarchy") => println!("Usage: hierarchy STORE OPERATOR SCTID [--display]\n\nOperators: <  <<  <!  <<!  >  >>  >!  >>!\nQuote the operator. Output is code lines, or JSONL with --display.\nUse expand for full expressions and terminal display tables."),
        Some(_) => anyhow::bail!("Unknown command. Run --help for available commands"),
    }
    Ok(())
}
