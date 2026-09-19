use anyhow::{bail, ensure, Context, Result};
use snomed_rust_ecl_engine::import::{import_snapshot, ImportOptions, UK_DISPLAY_REFSETS};
use snomed_rust_ecl_engine::store::{DisplayStore, Manifest, NumericStore};
use std::io::{self, Write};
use std::path::Path;
use std::time::Instant;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
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
            let manifest = import_snapshot(
                Path::new(&args[1]),
                Path::new(&args[2]),
                &ImportOptions {
                    edition: args[3].clone(),
                    expected_sha256: args[4].clone(),
                    display_refsets: refsets,
                },
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"manifest": manifest, "elapsed_seconds": start.elapsed().as_secs_f64()})
                )?
            );
        }
        Some("stats") => {
            ensure!(args.len() == 2, "Usage: stats STORE");
            println!(
                "{}",
                serde_json::to_string_pretty(&Manifest::read(Path::new(&args[1]))?)?
            );
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
                } else {
                    writeln!(out, "{code}")?;
                }
            }
        }
        _ => bail!("Commands: import, stats, hierarchy. See docs/compact-store.md."),
    }
    Ok(())
}
