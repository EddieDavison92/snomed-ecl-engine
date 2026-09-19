use anyhow::{bail, ensure, Context, Result};
use snomed_rust_ecl_engine::import::{import_snapshot, ImportOptions, UK_DISPLAY_REFSETS};
use snomed_rust_ecl_engine::store::{DisplayStore, Manifest, NumericStore};
use snomed_rust_ecl_engine::{ecl, eval};
use std::io::{self, BufRead, Read, Write};
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
            let expression = ecl::parse(&args[2])?;
            let store = NumericStore::open(Path::new(&args[1]))?;
            let ordinals = eval::evaluate(&store, &expression)?;
            if option == Some("--count") {
                println!("{}", ordinals.len());
            } else {
                let mut display = if option == Some("--display") {
                    Some(DisplayStore::open(Path::new(&args[1]))?)
                } else {
                    None
                };
                let mut out = io::BufWriter::new(io::stdout().lock());
                for ordinal in ordinals {
                    let code = store.ids[ordinal as usize];
                    if let Some(display) = &mut display {
                        writeln!(
                            out,
                            "{}",
                            serde_json::json!({"code": code.to_string(), "display": display.get(ordinal)?})
                        )?;
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
            let store = NumericStore::open(directory)?;
            let manifest = Manifest::read(directory)?;
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
                    Ok(request) => batch_response(&store, &manifest.edition, &request, &mut out)?,
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
                } else {
                    writeln!(out, "{code}")?;
                }
            }
        }
        _ => bail!("Commands: import, stats, hierarchy, expand, batch. See docs/basic-ecl.md."),
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
    total: usize,
    parse_ms: f64,
    eval_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    codes: Option<Codes<'a>>,
}

fn batch_response(
    store: &NumericStore,
    edition: &str,
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
    let ordinals = match eval::evaluate(store, &expression) {
        Ok(ordinals) => ordinals,
        Err(error) => {
            writeln!(out, "{}", serde_json::json!({"error":format!("{error:?}")}))?;
            return Ok(());
        }
    };
    let eval_ms = start.elapsed().as_secs_f64() * 1000.0;
    serde_json::to_writer(
        &mut *out,
        &BatchResponse {
            edition,
            total: ordinals.len(),
            parse_ms,
            eval_ms,
            codes: (!request.count_only).then_some(Codes {
                store,
                ordinals: &ordinals,
            }),
        },
    )?;
    writeln!(out)?;
    Ok(())
}
