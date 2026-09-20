use anyhow::{bail, ensure, Context, Result};
#[cfg(feature = "import")]
use snomed_ecl_engine::import::{import_snapshot_with_progress, ImportOptions, UK_DISPLAY_REFSETS};
use snomed_ecl_engine::store::{DisplayStore, Manifest, NumericStore};
use snomed_ecl_engine::{ecl, eval};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
mod presentation;
mod workspace;

/// Parses ECL, reporting the offending text under the expression rather than a
/// byte offset alone.
fn parse(expression: &str) -> Result<ecl::Expr> {
    ecl::parse(expression).map_err(|error| {
        anyhow::anyhow!(
            "{error}\n{}\nECL byte offset: {}. See docs/conformance.md for current support.",
            presentation::caret(expression, error.offset),
            error.offset
        )
    })
}

fn main() {
    if let Err(error) = run() {
        if error
            .downcast_ref::<io::Error>()
            .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
        {
            return;
        }
        eprintln!(
            "Error: {}",
            presentation::clean_message(&format!("{error:#}"))
        );
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let mut query_config = None;
    if let Some(position) = args.iter().position(|s| s == "--config") {
        ensure!(
            position + 1 < args.len(),
            "--config requires a JSON file path"
        );
        query_config = Some(snomed_ecl_engine::config::QueryConfig::read(Path::new(
            &args[position + 1],
        ))?);
        args.drain(position..=position + 1);
        ensure!(
            !args.iter().any(|s| s == "--config"),
            "--config may only be supplied once"
        );
    }
    let json = args.iter().any(|s| s == "--json");
    let plain = args.iter().any(|s| s == "--plain");
    ensure!(!(json && plain), "Choose either --json or --plain");
    args.retain(|s| s != "--json" && s != "--plain");
    let human = presentation::human() && !json && !plain;
    if args.is_empty()
        || matches!(
            args.first().map(String::as_str),
            Some("--help" | "-h" | "help")
        )
    {
        return presentation::help(args.get(1).map(String::as_str));
    }
    if args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h") {
        return presentation::help(Some(&args[0]));
    }
    if args.len() == 1 && matches!(args[0].as_str(), "--version" | "-V") {
        println!("snomed-ecl-engine {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let command = args.first().cloned().unwrap_or_default();
    match command.as_str() {
        "use" => {
            ensure!(args.len() == 2, "Usage: use STORE | use --clear");
            if args[1] == "--clear" {
                workspace::save(&workspace::State::default())?;
                println!("Cleared the selected index.");
                return Ok(());
            }
            // Store an absolute path so the selection survives a change of
            // working directory, and reject anything that is not an index.
            let path = std::fs::canonicalize(&args[1])
                .with_context(|| format!("No such path: {}", args[1]))?;
            let manifest = Manifest::read(&path).with_context(|| {
                format!(
                    "{} is not an index. `stores` lists the indexes it can find",
                    path.display()
                )
            })?;
            workspace::save(&workspace::State {
                store: Some(path.clone()),
            })?;
            if human {
                let location = format!("{} (now selected)", path.display());
                presentation::manifest(&manifest, Some(&location));
            } else {
                println!(
                    "{}",
                    serde_json::json!({"store": path, "edition": manifest.edition})
                );
            }
        }
        "stores" => {
            let roots: Vec<_> = if args.len() > 1 {
                args[1..].iter().map(PathBuf::from).collect()
            } else {
                workspace::default_roots()
            };
            let selected = workspace::load().store;
            let found = workspace::discover(&roots, selected.as_deref());
            if human {
                presentation::stores(&found);
            } else {
                for entry in &found {
                    println!(
                        "{}",
                        serde_json::json!({
                            "store": entry.path,
                            "edition": entry.edition,
                            "active_concepts": entry.active_concepts,
                            "concepts": entry.concepts,
                            "bytes": entry.bytes,
                            "packed": entry.packed,
                            "selected": entry.selected,
                        })
                    );
                }
            }
        }
        #[cfg(not(feature = "import"))]
        "inspect" => bail!("Archive inspection needs the importer; rebuild with --features import"),
        #[cfg(feature = "import")]
        "inspect" => {
            ensure!(args.len() == 2, "Usage: inspect ARCHIVE");
            let summary = snomed_ecl_engine::import::inspect_archive(Path::new(&args[1]))?;
            if !human {
                println!("{}", serde_json::to_string_pretty(&summary)?);
                return Ok(());
            }
            println!("{}", presentation::heading("SNOMED ECL / archive"));
            println!();
            println!("  File      {}", presentation::clean(&args[1]));
            println!("  Size      {}", presentation::bytes(summary.bytes));
            println!("  SHA-256   {}", summary.sha256);
            println!(
                "  Release   {}",
                presentation::clean(&summary.effective_time)
            );
            println!();
            for (label, found) in &summary.required_files {
                match found {
                    Some(name) => println!("  found     {}", presentation::clean(name)),
                    None => println!("  MISSING   {label}"),
                }
            }
            if !summary.importable {
                println!();
                println!("  Required Snapshot files are missing. The importer takes one");
                println!("  self-contained Snapshot package; Full, Delta and split");
                println!("  extensions are not supported.");
                return Ok(());
            }
            println!();
            if summary.edition_uris.is_empty() {
                println!("  No module declares this release date, so no edition URI can be");
                println!("  offered. Use the versioned URI from the release distributor.");
                return Ok(());
            }
            match summary.root_editions {
                1 => println!("  Edition URI"),
                0 => println!("  Edition URI candidates (no single root module)"),
                n => println!("  Edition URI candidates ({n} root modules)"),
            }
            for (index, uri) in summary.edition_uris.iter().enumerate() {
                let marker = if index < summary.root_editions.max(1) {
                    " "
                } else {
                    "-"
                };
                println!("  {marker} {}", presentation::clean(uri));
            }
            if summary.edition_uris.len() > summary.root_editions.max(1) {
                println!();
                println!("  Lines marked - are modules another module in this package depends");
                println!("  on, so they are components of the edition rather than the edition.");
            }
            println!();
            println!("  Check the SHA-256 above against the value the release distributor");
            println!("  published. A checksum taken from the downloaded file shows only that");
            println!("  the file is intact, never where it came from. Then import:");
            println!();
            println!(
                "    snomed-ecl-engine import {} INDEX_DIRECTORY \\",
                presentation::clean(&args[1])
            );
            println!("      {} \\", presentation::clean(&summary.edition_uris[0]));
            println!("      {}", summary.sha256);
        }
        "query" => {
            let mut style = Style::take(&mut args, human, json)?;
            ensure!(args.len() <= 2, "Usage: query [STORE] [--display|--count]");
            let (path, source) = workspace::resolve(args.get(1).map(String::as_str))?;
            let open_start = Instant::now();
            let mut store = NumericStore::open(&path)?;
            if let Some(config) = &query_config {
                store.config = config.clone();
            }
            let manifest = Manifest::read(&path)?;
            let mut display = None;
            println!("{}\n", presentation::heading("SNOMED ECL / query"));
            println!("  Index    {} ({})", path.display(), source.describe());
            println!("  Edition  {}", presentation::clean(&manifest.edition));
            println!(
                "  Opened   {:.2}s, {} active concepts",
                open_start.elapsed().as_secs_f64(),
                presentation::number(manifest.active_concept_count)
            );
            println!("\n  Type an ECL expression, or :help for commands. :quit exits.\n");
            let mut input = io::stdin().lock();
            let mut line = String::new();
            loop {
                print!("ecl> ");
                io::stdout().flush()?;
                line.clear();
                if input.read_line(&mut line)? == 0 {
                    println!();
                    break;
                }
                let text = line.trim();
                if text.is_empty() {
                    continue;
                }
                match text {
                    ":quit" | ":q" | ":exit" => break,
                    ":help" | ":h" => {
                        println!(
                            "  :display  toggle result terms (currently {})\n  \
                             :count    toggle totals only (currently {})\n  \
                             :stats    show the index manifest\n  \
                             :quit     leave\n\n  \
                             Anything else is evaluated as ECL. Results list {} at a time.",
                            if style.display { "on" } else { "off" },
                            if style.count { "on" } else { "off" },
                            QUERY_LIMIT
                        );
                        continue;
                    }
                    ":display" => {
                        style.display = !style.display;
                        style.count &= !style.display;
                        println!("  Terms {}.", if style.display { "on" } else { "off" });
                        continue;
                    }
                    ":count" => {
                        style.count = !style.count;
                        style.display &= !style.count;
                        println!("  Totals only {}.", if style.count { "on" } else { "off" });
                        continue;
                    }
                    ":stats" => {
                        presentation::manifest(&manifest, Some(&path.display().to_string()));
                        continue;
                    }
                    _ => {}
                }
                // One bad expression must not end the session, so parse and
                // evaluation errors are reported and the prompt returns.
                let start = Instant::now();
                let outcome = parse(text)
                    .and_then(|expression| Ok(eval::evaluate_result(&store, &expression)?));
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                match outcome {
                    Ok(result) => {
                        println!("  {} in {elapsed:.3} ms", presentation::total(&result));
                        let mut out = io::BufWriter::new(io::stdout().lock());
                        let shown = emit(
                            &store,
                            &path,
                            &mut display,
                            &result,
                            &style,
                            Some(QUERY_LIMIT),
                            &mut out,
                        );
                        out.flush()?;
                        if let Err(error) = shown {
                            println!("  {}", presentation::clean_message(&format!("{error:#}")));
                        }
                    }
                    Err(error) => {
                        println!("{}", presentation::clean_message(&format!("{error:#}")))
                    }
                }
                println!();
            }
        }
        "diff" => {
            let style = Style::take(&mut args, human, json)?;
            ensure!(
                args.len() == 4,
                "Usage: diff OLD_STORE NEW_STORE ECL [--display|--count]"
            );
            let expression = parse(&args[3])?;
            let (old, new) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
            // Each index is opened, evaluated and dropped in turn so only one is
            // resident at a time.
            let (old_edition, old_codes) = evaluate_for_diff(&old, &expression, &query_config)?;
            let (new_edition, new_codes) = evaluate_for_diff(&new, &expression, &query_config)?;
            let removed: Vec<_> = old_codes
                .iter()
                .filter(|(code, _)| new_codes.binary_search_by_key(code, |(c, _)| *c).is_err())
                .copied()
                .collect();
            let added: Vec<_> = new_codes
                .iter()
                .filter(|(code, _)| old_codes.binary_search_by_key(code, |(c, _)| *c).is_err())
                .copied()
                .collect();
            let unchanged = old_codes.len() - removed.len();
            if !human {
                let mut out = io::BufWriter::new(io::stdout().lock());
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({
                        "ecl": args[3],
                        "old": {"store": old, "edition": old_edition, "total": old_codes.len()},
                        "new": {"store": new, "edition": new_edition, "total": new_codes.len()},
                        "unchanged": unchanged,
                        "added": added.iter().map(|(c, _)| c.to_string()).collect::<Vec<_>>(),
                        "removed": removed.iter().map(|(c, _)| c.to_string()).collect::<Vec<_>>(),
                    })
                )?;
                out.flush()?;
                return Ok(());
            }
            println!("{}\n", presentation::heading("SNOMED ECL / diff"));
            println!("  Expression  {}", presentation::clean(&args[3]));
            println!(
                "  Old         {} ({}), {} results",
                old.display(),
                presentation::clean(&old_edition),
                presentation::number(old_codes.len())
            );
            println!(
                "  New         {} ({}), {} results",
                new.display(),
                presentation::clean(&new_edition),
                presentation::number(new_codes.len())
            );
            println!(
                "\n  {} unchanged, {} added, {} removed",
                presentation::number(unchanged),
                presentation::number(added.len()),
                presentation::number(removed.len())
            );
            if style.count {
                return Ok(());
            }
            print_diff_side("Added", &added, &new, style.display)?;
            print_diff_side("Removed", &removed, &old, style.display)?;
            if added.is_empty() && removed.is_empty() {
                println!("\n  The expression selects the same concepts in both indexes.");
            }
        }
        "pack" => {
            ensure!(
                (3..=5).contains(&args.len()),
                "Usage: pack STORE DESTINATION_FILE [--uncompressed|--block-kib 16|64]"
            );
            let mut options = snomed_ecl_engine::store::PackOptions::default();
            match args.get(3).map(String::as_str) {
                None => {}
                Some("--uncompressed") if args.len() == 4 => options.compress = false,
                Some("--block-kib") if args.len() == 5 => {
                    options.block_bytes = args[4]
                        .parse::<u32>()?
                        .checked_mul(1024)
                        .context("Block size overflow")?;
                }
                _ => bail!("Usage: pack STORE DESTINATION_FILE [--uncompressed|--block-kib 16|64]"),
            }
            let start = Instant::now();
            snomed_ecl_engine::store::pack_with_options(
                Path::new(&args[1]),
                Path::new(&args[2]),
                options,
            )?;
            let bytes = std::fs::metadata(&args[2])?.len();
            if human {
                println!(
                    "Packed index: {} ({:.2} MiB)",
                    presentation::clean(&args[2]),
                    bytes as f64 / 1_048_576.0
                );
            } else {
                println!(
                    "{}",
                    serde_json::json!({"bytes":bytes,"elapsed_seconds":start.elapsed().as_secs_f64()})
                );
            }
        }
        "verify" => {
            ensure!(args.len() <= 2, "Usage: verify [STORE]");
            let (store, _) = workspace::resolve(args.get(1).map(String::as_str))?;
            let start = Instant::now();
            let result = snomed_ecl_engine::store::verify(&store)?;
            if human {
                println!(
                    "Verified {} sections and {} concepts in {:.2}s",
                    result.sections,
                    presentation::number(result.concepts),
                    start.elapsed().as_secs_f64()
                );
            } else {
                println!("{}", serde_json::to_string(&result)?);
            }
        }
        #[cfg(not(feature = "import"))]
        "import" | "add-refsets" => {
            bail!("Import support was excluded; rebuild with --features import")
        }
        #[cfg(feature = "import")]
        "add-refsets" => {
            ensure!(
                args.len() == 6,
                "Usage: add-refsets BASE_STORE ARCHIVE DESTINATION RELEASE_DATE SHA256"
            );
            let start = Instant::now();
            eprintln!("  Verifying and adding supplementary refsets...");
            let manifest = snomed_ecl_engine::import::add_refsets_snapshot(
                Path::new(&args[1]),
                Path::new(&args[2]),
                Path::new(&args[3]),
                &args[4],
                &args[5],
            )?;
            if human {
                presentation::manifest(&manifest, Some(&args[3]));
            } else {
                println!("{}", serde_json::to_string_pretty(&manifest)?);
            }
            eprintln!(
                "  Supplement complete in {:.2}s",
                start.elapsed().as_secs_f64()
            );
        }
        #[cfg(feature = "import")]
        "import" => {
            ensure!(
                args.len() == 5 || args.len() == 6,
                "Usage: import ARCHIVE DESTINATION EDITION_URI SHA256 [DISPLAY_REFSET_IDS]"
            );
            let refsets = if args.len() == 6 {
                args[5]
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                UK_DISPLAY_REFSETS.to_vec()
            };
            let start = Instant::now();
            let mut stage = 0;
            let manifest = import_snapshot_with_progress(
                Path::new(&args[1]),
                Path::new(&args[2]),
                &ImportOptions {
                    edition: args[3].clone(),
                    expected_sha256: args[4].clone(),
                    display_refsets: refsets,
                },
                |message| {
                    stage += 1;
                    eprintln!(
                        "  [{stage}/9] {message}  ({:.1}s elapsed)",
                        start.elapsed().as_secs_f64()
                    );
                },
            )?;
            if human {
                presentation::manifest(&manifest, Some(&args[2]));
                eprintln!(
                    "\n  Import complete in {:.2}s",
                    start.elapsed().as_secs_f64()
                );
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"manifest": manifest, "elapsed_seconds": start.elapsed().as_secs_f64()})
                    )?
                );
            }
        }
        "stats" => {
            ensure!(args.len() <= 2, "Usage: stats [STORE]");
            let (store, source) = workspace::resolve(args.get(1).map(String::as_str))?;
            let manifest = Manifest::read(&store)
                .with_context(|| format!("Cannot read the index at {}", store.display()))?;
            if human {
                let location = format!("{} ({})", store.display(), source.describe());
                presentation::manifest(&manifest, Some(&location));
            } else {
                println!("{}", serde_json::to_string_pretty(&manifest)?);
            }
        }
        "expand" => {
            let style = Style::take(&mut args, human, json)?;
            ensure!(
                args.len() == 2 || args.len() == 3,
                "Usage: expand [STORE] ECL [--display|--count]"
            );
            // The expression is always last; a preceding argument names the index.
            let text = args[args.len() - 1].clone();
            let explicit = (args.len() == 3).then(|| args[1].as_str());
            let (path, _) = workspace::resolve(explicit)?;
            let parse_start = Instant::now();
            let expression = parse(&text)?;
            let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
            if human {
                eprintln!("  Opening and verifying index...");
            }
            let open_start = Instant::now();
            let mut store = NumericStore::open(&path)?;
            if let Some(config) = &query_config {
                store.config = config.clone();
            }
            let open_seconds = open_start.elapsed().as_secs_f64();
            let eval_start = Instant::now();
            let result = eval::evaluate_result(&store, &expression)?;
            let eval_ms = eval_start.elapsed().as_secs_f64() * 1000.0;
            if human {
                eprintln!(
                    "  {} | query {:.3} ms | parse {:.3} ms | index {:.3} s",
                    presentation::total(&result),
                    eval_ms,
                    parse_ms,
                    open_seconds
                );
            }
            let mut display = None;
            let mut out = io::BufWriter::new(io::stdout().lock());
            // A terminal lists a page; redirected output and --json stay complete,
            // so scripts are unaffected and the total is always reported.
            let limit = human.then_some(EXPAND_LIMIT);
            emit(
                &store,
                &path,
                &mut display,
                &result,
                &style,
                limit,
                &mut out,
            )?;
            out.flush()?;
        }
        "batch" => {
            ensure!(
                args.len() <= 2,
                "Usage: batch [STORE] (JSON lines on stdin)"
            );
            let (directory, _) = workspace::resolve(args.get(1).map(String::as_str))?;
            let start = Instant::now();
            let mut store = NumericStore::open(&directory)?;
            if let Some(config) = &query_config {
                store.config = config.clone();
            }
            let manifest = Manifest::read(&directory)?;
            let config_sha256 = store.config.fingerprint()?;
            eprintln!(
                "Store opened in {:.3} seconds",
                start.elapsed().as_secs_f64()
            );
            let mut input = io::stdin().lock();
            let mut out = io::BufWriter::new(io::stdout().lock());
            let mut line = Vec::new();
            loop {
                line.clear();
                if (&mut input).take(524289).read_until(b'\n', &mut line)? == 0 {
                    break;
                }
                ensure!(line.len() <= 524288, "Batch request exceeds 512 KiB");
                match serde_json::from_slice::<BatchRequest>(&line) {
                    Ok(request) => {
                        batch_response(&store, &manifest, &config_sha256, &request, &mut out)?
                    }
                    Err(_) => writeln!(out, "{{\"error\":\"InvalidRequest\"}}")?,
                }
                out.flush()?;
            }
        }
        "hierarchy" => {
            let with_display = args.iter().any(|s| s == "--display");
            args.retain(|s| s != "--display");
            ensure!(
                args.len() == 3 || args.len() == 4,
                "Usage: hierarchy [STORE] OPERATOR SCTID [--display]"
            );
            // The operator and SCTID are always last; a preceding argument names
            // the index.
            let (operator, sctid) = (&args[args.len() - 2], &args[args.len() - 1]);
            let (ancestors, direct, include_self) = match operator.as_str() {
                "<" => (false, false, false),
                "<<" => (false, false, true),
                "<!" => (false, true, false),
                "<<!" => (false, true, true),
                ">" => (true, false, false),
                ">>" => (true, false, true),
                ">!" => (true, true, false),
                ">>!" => (true, true, true),
                _ => bail!("Unsupported hierarchy operator; this command is not an ECL parser"),
            };
            let explicit = (args.len() == 4).then(|| args[1].as_str());
            let (path, _) = workspace::resolve(explicit)?;
            let store = NumericStore::open(&path)?;
            let codes = store.hierarchy(sctid.parse()?, ancestors, direct, include_self);
            let mut displays = if with_display {
                Some(DisplayStore::open(&path)?)
            } else {
                None
            };
            let mut out = io::BufWriter::new(io::stdout().lock());
            for code in codes {
                if let Some(ref mut display) = displays {
                    let text =
                        display.get(store.ordinal(code).context("Missing result concept")?)?;
                    writeln!(
                        out,
                        "{}",
                        serde_json::json!({"code": code.to_string(), "display": text})
                    )?;
                } else if json {
                    writeln!(out, "{}", serde_json::json!({"code": code.to_string()}))?;
                } else {
                    writeln!(out, "{code}")?;
                }
            }
            out.flush()?;
        }
        _ => bail!("Unknown command. Run --help for available commands"),
    }
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchRequest {
    ecl: String,
    #[serde(default)]
    count_only: bool,
}

struct Codes<'a> {
    store: &'a NumericStore,
    ordinals: &'a [u32],
}
impl serde::Serialize for Codes<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.ordinals.len()))?;
        for &ordinal in self.ordinals {
            seq.serialize_element(&self.store.ids[ordinal as usize].to_string())?;
        }
        seq.end()
    }
}

#[derive(serde::Serialize)]
struct BatchResponse<'a> {
    edition: &'a str,
    query_config_sha256: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    supplements: Vec<&'a str>,
    total: usize,
    parse_ms: f64,
    eval_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    codes: Option<Codes<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rows: Option<&'a [std::collections::BTreeMap<String, snomed_ecl_engine::store::MemberValue>]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    values: Option<&'a [snomed_ecl_engine::store::MemberValue]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_type: Option<&'static str>,
}

fn batch_response(
    store: &NumericStore,
    manifest: &Manifest,
    config_sha256: &str,
    request: &BatchRequest,
    out: &mut impl Write,
) -> Result<()> {
    let start = Instant::now();
    let expression = match ecl::parse(&request.ecl) {
        Ok(expression) => expression,
        Err(error) => {
            writeln!(
                out,
                "{}",
                serde_json::json!({"error":format!("{:?}", error.kind), "offset":error.offset, "message":error.message})
            )?;
            return Ok(());
        }
    };
    let parse_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let result = match eval::evaluate_result(store, &expression) {
        Ok(result) => result,
        Err(error) => {
            writeln!(out, "{}", serde_json::json!({"error":format!("{error:?}")}))?;
            return Ok(());
        }
    };
    let eval_ms = start.elapsed().as_secs_f64() * 1000.0;
    serde_json::to_writer(
        &mut *out,
        &BatchResponse {
            edition: &manifest.edition,
            query_config_sha256: config_sha256,
            supplements: manifest
                .supplements
                .iter()
                .map(|s| s.archive_sha256.as_str())
                .collect(),
            total: result.len(),
            parse_ms,
            eval_ms,
            codes: match &result {
                eval::QueryResult::Concepts(ordinals) if !request.count_only => {
                    Some(Codes { store, ordinals })
                }
                _ => None,
            },
            rows: match &result {
                eval::QueryResult::Rows(rows) if !request.count_only => Some(rows),
                _ => None,
            },
            values: match &result {
                eval::QueryResult::Values(values) if !request.count_only => Some(values),
                _ => None,
            },
            result_type: match result {
                eval::QueryResult::Rows(_) => Some("rows"),
                eval::QueryResult::Values(_) => Some("values"),
                _ => None,
            },
        },
    )?;
    writeln!(out)?;
    Ok(())
}

/// Codes listed per result in the interactive loop. The total is always
/// printed, so this caps the listing only.
const QUERY_LIMIT: usize = 40;

/// Codes listed by `expand` in a terminal, for the same reason. Redirected
/// output is never capped.
const EXPAND_LIMIT: usize = 200;

/// Codes listed per side of a terminal diff, for the same reason.
const DIFF_LIMIT: usize = 40;

/// Evaluates one expression against one index and returns its edition with the
/// result as sorted (code, ordinal) pairs. The ordinal is kept so terms can be
/// resolved later without reopening the numeric index.
fn evaluate_for_diff(
    path: &Path,
    expression: &ecl::Expr,
    config: &Option<snomed_ecl_engine::config::QueryConfig>,
) -> Result<(String, Vec<(u64, u32)>)> {
    let mut store = NumericStore::open(path)
        .with_context(|| format!("Cannot open the index at {}", path.display()))?;
    if let Some(config) = config {
        store.config = config.clone();
    }
    let manifest = Manifest::read(path)?;
    let result = eval::evaluate_result(&store, expression)?;
    let eval::QueryResult::Concepts(ordinals) = result else {
        bail!("diff compares concept results; this projection returns values or rows")
    };
    let mut codes: Vec<_> = ordinals
        .into_iter()
        .map(|ordinal| (store.ids[ordinal as usize], ordinal))
        .collect();
    codes.sort_unstable();
    Ok((manifest.edition, codes))
}

/// Prints one side of a diff, resolving terms from the index that side came
/// from. A concept removed by a release exists only in the older index.
fn print_diff_side(
    label: &str,
    codes: &[(u64, u32)],
    path: &Path,
    with_display: bool,
) -> Result<()> {
    if codes.is_empty() {
        return Ok(());
    }
    println!(
        "\n  {} ({})",
        presentation::heading(label),
        presentation::number(codes.len())
    );
    let mut display = if with_display {
        Some(DisplayStore::open(path)?)
    } else {
        None
    };
    let shown = codes.len().min(DIFF_LIMIT);
    for &(code, ordinal) in &codes[..shown] {
        match display.as_mut() {
            Some(display) => println!(
                "    {code:<20}  {}",
                display
                    .get(ordinal)?
                    .as_deref()
                    .map(presentation::clean)
                    .as_deref()
                    .unwrap_or("(no display)")
            ),
            None => println!("    {code}"),
        }
    }
    if shown < codes.len() {
        println!(
            "    ... {} more. Use --json for every code.",
            presentation::number(codes.len() - shown)
        );
    }
    Ok(())
}

/// Output flags shared by `expand`, `query` and `diff`.
struct Style {
    display: bool,
    count: bool,
    json: bool,
    human: bool,
}

impl Style {
    /// Removes the output flags so the remaining arguments are positional.
    fn take(args: &mut Vec<String>, human: bool, json: bool) -> Result<Self> {
        let display = args.iter().any(|s| s == "--display");
        let count = args.iter().any(|s| s == "--count");
        ensure!(!(display && count), "Choose either --display or --count");
        args.retain(|s| s != "--display" && s != "--count");
        if let Some(unknown) = args.iter().skip(1).find(|s| s.starts_with("--")) {
            bail!("Unknown option {unknown}. Run `{} --help`", args[0]);
        }
        Ok(Self {
            display,
            count,
            json,
            human,
        })
    }
}

/// Writes one query result. `limit` caps the codes listed in a terminal; the
/// full total is always reported alongside, so a capped listing is never
/// mistaken for the complete set.
fn emit(
    store: &NumericStore,
    path: &Path,
    display: &mut Option<DisplayStore>,
    result: &eval::QueryResult,
    style: &Style,
    limit: Option<usize>,
    out: &mut impl Write,
) -> Result<()> {
    let ordinals = match result {
        eval::QueryResult::Concepts(ordinals) => ordinals,
        eval::QueryResult::Values(values) => {
            ensure!(
                !style.display,
                "--display requires a concept result; this projection returns scalar values"
            );
            if style.count {
                if style.json {
                    writeln!(
                        out,
                        "{}",
                        serde_json::json!({"total": values.len(), "result_type":"values"})
                    )?;
                } else {
                    writeln!(out, "{}", values.len())?;
                }
            } else {
                for value in values {
                    serde_json::to_writer(&mut *out, value)?;
                    writeln!(out)?;
                }
            }
            return Ok(());
        }
        eval::QueryResult::Rows(rows) => {
            ensure!(
                !style.display,
                "--display requires a concept result; this projection returns rows"
            );
            if style.count {
                if style.json {
                    writeln!(
                        out,
                        "{}",
                        serde_json::json!({"total": rows.len(), "result_type":"rows"})
                    )?;
                } else {
                    writeln!(out, "{}", rows.len())?;
                }
            } else {
                for row in rows {
                    serde_json::to_writer(&mut *out, row)?;
                    writeln!(out)?;
                }
            }
            return Ok(());
        }
    };
    if style.count {
        if style.json {
            writeln!(out, "{}", serde_json::json!({"total": ordinals.len()}))?;
        } else {
            writeln!(out, "{}", ordinals.len())?;
        }
        return Ok(());
    }
    if style.display && display.is_none() {
        *display = Some(DisplayStore::open(path)?);
    }
    let shown = limit.unwrap_or(ordinals.len()).min(ordinals.len());
    if style.human && style.display {
        writeln!(out, "\n{}\n", presentation::heading("SNOMED ECL / results"))?;
        writeln!(out, "{:<20}  DISPLAY", "CODE")?;
        writeln!(out, "{}", "-".repeat(64))?;
    }
    for &ordinal in &ordinals[..shown] {
        let code = store.ids[ordinal as usize];
        if style.display {
            let label = display
                .as_mut()
                .context("Display index was not opened")?
                .get(ordinal)?;
            if style.human {
                writeln!(
                    out,
                    "{code:<20}  {}",
                    label
                        .as_deref()
                        .map(presentation::clean)
                        .as_deref()
                        .unwrap_or("(no display)")
                )?;
            } else {
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({"code": code.to_string(), "display": label})
                )?;
            }
        } else if style.json {
            writeln!(out, "{}", serde_json::json!({"code": code.to_string()}))?;
        } else {
            writeln!(out, "{code}")?;
        }
    }
    if shown < ordinals.len() {
        writeln!(
            out,
            "\n  Listed {} of {}. Redirect output or use --json for every code.",
            presentation::number(shown),
            presentation::number(ordinals.len())
        )?;
    }
    Ok(())
}
