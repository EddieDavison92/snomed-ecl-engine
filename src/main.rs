use anyhow::{bail, ensure, Context, Result};
#[cfg(feature = "import")]
use snomed_ecl_engine::import::{import_snapshot_with_progress, ImportOptions, UK_DISPLAY_REFSETS};
use snomed_ecl_engine::store::{DisplayStore, Manifest, NumericStore};
use snomed_ecl_engine::{ecl, eval};
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::time::Instant;
mod presentation;

fn main() {
    if let Err(error) = run() {
        if error
            .downcast_ref::<io::Error>()
            .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
        {
            return;
        }
        eprintln!("Error: {}", presentation::clean(&format!("{error:#}")));
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
    match args.first().map(String::as_str) {
        Some("pack") => {
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
        Some("verify") => {
            ensure!(args.len() == 2, "Usage: verify STORE");
            let start = Instant::now();
            let result = snomed_ecl_engine::store::verify(Path::new(&args[1]))?;
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
        Some("import" | "add-refsets") => {
            bail!("Import support was excluded; rebuild with --features import")
        }
        #[cfg(feature = "import")]
        Some("add-refsets") => {
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
                presentation::manifest(&manifest);
            } else {
                println!("{}", serde_json::to_string_pretty(&manifest)?);
            }
            eprintln!(
                "  Supplement complete in {:.2}s",
                start.elapsed().as_secs_f64()
            );
        }
        #[cfg(feature = "import")]
        Some("import") => {
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
                presentation::manifest(&manifest);
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
        Some("stats") => {
            ensure!(args.len() == 2, "Usage: stats STORE");
            let manifest = Manifest::read(Path::new(&args[1]))?;
            if human {
                presentation::manifest(&manifest);
            } else {
                println!("{}", serde_json::to_string_pretty(&manifest)?);
            }
        }
        Some("expand") => {
            ensure!(
                args.len() == 3 || args.len() == 4,
                "Usage: expand STORE ECL [--display|--count]"
            );
            let option = args.get(3).map(String::as_str);
            ensure!(
                matches!(option, None | Some("--display" | "--count")),
                "Unknown expansion option"
            );
            let parse_start = Instant::now();
            let expression = ecl::parse(&args[2]).map_err(|error| {
                anyhow::anyhow!(
                    "{error}. ECL byte offset: {}. See docs/conformance.md for current support.",
                    error.offset
                )
            })?;
            let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
            if human {
                eprintln!("  Opening and verifying index...");
            }
            let open_start = Instant::now();
            let mut store = NumericStore::open(Path::new(&args[1]))?;
            if let Some(config) = &query_config {
                store.config = config.clone();
            }
            let open_seconds = open_start.elapsed().as_secs_f64();
            let eval_start = Instant::now();
            let result = eval::evaluate_result(&store, &expression)?;
            let eval_ms = eval_start.elapsed().as_secs_f64() * 1000.0;
            let ordinals = match result {
                eval::QueryResult::Concepts(ordinals) => ordinals,
                eval::QueryResult::Values(values) => {
                    ensure!(option != Some("--display"), "--display requires a concept result; this projection returns scalar values");
                    if human {
                        eprintln!(
                            "  {} values | query {:.3} ms | parse {:.3} ms | index {:.3} s",
                            presentation::number(values.len()),
                            eval_ms,
                            parse_ms,
                            open_seconds
                        );
                    }
                    if option == Some("--count") {
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({"total": values.len(), "result_type":"values"})
                            );
                        } else {
                            println!("{}", values.len());
                        }
                    } else {
                        let mut out = io::BufWriter::new(io::stdout().lock());
                        for value in values {
                            serde_json::to_writer(&mut out, &value)?;
                            writeln!(out)?;
                        }
                        out.flush()?;
                    }
                    return Ok(());
                }
                eval::QueryResult::Rows(rows) => {
                    ensure!(
                        option != Some("--display"),
                        "--display requires a concept result; this projection returns rows"
                    );
                    if human {
                        eprintln!(
                            "  {} rows | query {:.3} ms | parse {:.3} ms | index {:.3} s",
                            presentation::number(rows.len()),
                            eval_ms,
                            parse_ms,
                            open_seconds
                        );
                    }
                    if option == Some("--count") {
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({"total": rows.len(), "result_type":"rows"})
                            );
                        } else {
                            println!("{}", rows.len());
                        }
                    } else {
                        let mut out = io::BufWriter::new(io::stdout().lock());
                        for row in rows {
                            serde_json::to_writer(&mut out, &row)?;
                            writeln!(out)?;
                        }
                        out.flush()?;
                    }
                    return Ok(());
                }
            };
            if human {
                eprintln!(
                    "  {} concepts | query {:.3} ms | parse {:.3} ms | index {:.3} s",
                    presentation::number(ordinals.len()),
                    eval_ms,
                    parse_ms,
                    open_seconds
                );
            }
            if option == Some("--count") {
                if json {
                    println!("{}", serde_json::json!({"total": ordinals.len()}));
                } else {
                    println!("{}", ordinals.len());
                }
            } else {
                let mut display = if option == Some("--display") {
                    Some(DisplayStore::open(Path::new(&args[1]))?)
                } else {
                    None
                };
                let mut out = io::BufWriter::new(io::stdout().lock());
                if human && display.is_some() {
                    writeln!(out, "\n{}\n", presentation::heading("SNOMED ECL / results"))?;
                    writeln!(out, "{:<20}  DISPLAY", "CODE")?;
                    writeln!(out, "{}", "-".repeat(64))?;
                }
                for ordinal in ordinals {
                    let code = store.ids[ordinal as usize];
                    if let Some(display) = &mut display {
                        let label = display.get(ordinal)?;
                        if human {
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
                    } else if json {
                        writeln!(out, "{}", serde_json::json!({"code": code.to_string()}))?;
                    } else {
                        writeln!(out, "{code}")?;
                    }
                }
                out.flush()?;
            }
        }
        Some("batch") => {
            ensure!(args.len() == 2, "Usage: batch STORE (JSON lines on stdin)");
            let directory = Path::new(&args[1]);
            let start = Instant::now();
            let mut store = NumericStore::open(directory)?;
            if let Some(config) = &query_config {
                store.config = config.clone();
            }
            let manifest = Manifest::read(directory)?;
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
        Some("hierarchy") => {
            ensure!(
                args.len() == 4 || args.len() == 5,
                "Usage: hierarchy STORE OPERATOR SCTID [--display]"
            );
            if args.len() == 5 {
                ensure!(args[4] == "--display", "Unknown hierarchy option");
            }
            let (ancestors, direct, include_self) = match args[2].as_str() {
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
            let store = NumericStore::open(Path::new(&args[1]))?;
            let codes = store.hierarchy(args[3].parse()?, ancestors, direct, include_self);
            let mut displays = if args.len() == 5 {
                Some(DisplayStore::open(Path::new(&args[1]))?)
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
