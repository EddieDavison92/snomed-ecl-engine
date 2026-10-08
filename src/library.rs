//! The index library: packed indexes kept in one folder, named by edition and
//! release date, so they can be chosen as `uk@2026-08` rather than by path.
//!
//! Like the selection in `workspace`, this belongs to the CLI. The library
//! never changes what an index answers.
use anyhow::{bail, Context, Result};
use snomed_ecl_engine::store::Manifest;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const ENV_HOME: &str = "SNOMED_ECL_HOME";

/// Editions with a short name. Any other edition is named by its module ID.
const FAMILIES: &[(&str, &str)] = &[("83821000000107", "uk"), ("900000000000207008", "int")];

pub fn edition_family(module: &str) -> Option<&'static str> {
    FAMILIES
        .iter()
        .find(|(id, _)| *id == module)
        .map(|(_, name)| *name)
}

/// Prefer a known edition only when exactly one candidate names one.
#[cfg(feature = "import")]
pub fn choose_local_edition(summary: &snomed_ecl_engine::import::ArchiveSummary) -> Result<&str> {
    let mut modules = summary.edition_uris.iter().filter_map(|uri| {
        let rest = uri.strip_prefix("http://snomed.info/sct/")?;
        let (module, _) = rest.split_once("/version/")?;
        edition_family(module)?;
        module.parse().ok()
    });
    if let Some(module) = modules.next() {
        if modules.next().is_none() {
            return summary.choose_edition(Some(module));
        }
    }
    if summary.edition_uris.len() == 1 {
        return Ok(&summary.edition_uris[0]);
    }
    bail!(
        "The archive does not name one edition. Candidates: {}. Choose one with --edition URI",
        summary.edition_uris.join(", ")
    )
}

/// Where library indexes live: `SNOMED_ECL_HOME`, else the platform's data
/// folder. Created when an index is first added.
pub fn home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os(ENV_HOME).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
        })
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
    };
    Ok(base
        .context("Cannot locate a data folder for indexes; set SNOMED_ECL_HOME")?
        .join("snomed-ecl-engine")
        .join("indexes"))
}

/// The edition's short name and release date, from a URI such as
/// `http://snomed.info/sct/83821000000107/version/20260826`.
pub fn edition_parts(edition: &str) -> Option<(String, String)> {
    let rest = edition.strip_prefix("http://snomed.info/sct/")?;
    let (module, date) = rest.split_once("/version/")?;
    if date.len() != 8 || !date.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let family = edition_family(module).unwrap_or(module).to_owned();
    Some((family, date.to_owned()))
}

/// The name an index gets when none is given, such as `uk-20260826`.
#[cfg_attr(not(feature = "import"), allow(dead_code))]
pub fn default_name(edition: &str) -> Option<String> {
    edition_parts(edition).map(|(family, date)| format!("{family}-{date}"))
}

/// A release date shown as `2026-08-26`.
pub fn show_date(date: &str) -> String {
    if date.len() == 8 {
        format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..])
    } else {
        date.to_owned()
    }
}

/// Names are also file names, so they are kept to a portable alphabet.
#[cfg_attr(not(feature = "import"), allow(dead_code))]
pub fn check_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['.', '-'])
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !valid {
        bail!("Index names use letters, digits, '-', '_' and '.', up to 64 characters");
    }
    Ok(())
}

/// One index in the library.
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    /// Short edition name and release date, when the URI has the usual form.
    pub release: Option<(String, String)>,
    source_release_date: Option<String>,
    modified: Option<SystemTime>,
}

/// Every index in the library, by name.
pub fn entries() -> Vec<Entry> {
    let Ok(home) = home() else {
        return Vec::new();
    };
    entries_in(&home)
}

fn entries_in(home: &Path) -> Vec<Entry> {
    let Ok(listing) = std::fs::read_dir(home) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = listing
        .flatten()
        .map(|item| item.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ecl") && path.is_file())
        .filter_map(|path| {
            let name = path.file_stem()?.to_str()?.to_owned();
            let manifest = Manifest::read(&path).ok()?;
            let release = edition_parts(&manifest.edition);
            let source_release_date = manifest.source.map(|source| source.release_date);
            let modified = path
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok());
            Some(Entry {
                name,
                path,
                release,
                source_release_date,
                modified,
            })
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// Finds a library index by name (`uk-20260826`), by edition and release
/// (`uk@2026-08`, `uk@20260826`), or by edition alone (`uk`, the latest).
/// A partial date matches its latest release, so `uk@2026` is the newest of 2026.
pub fn find(reference: &str) -> Result<Option<PathBuf>> {
    find_among(reference, &entries())
}

fn find_among(reference: &str, entries: &[Entry]) -> Result<Option<PathBuf>> {
    if let Some(entry) = entries.iter().find(|entry| entry.name == reference) {
        return Ok(Some(entry.path.clone()));
    }
    let (family, date) = match reference.split_once('@') {
        Some((family, date)) => (family, date.replace('-', "")),
        None => (reference, String::new()),
    };
    if !date.is_empty() && date != "latest" && !date.bytes().all(|b| b.is_ascii_digit()) {
        bail!("Give a release as a date, such as {family}@2026-08-26 or {family}@2026-08");
    }
    let date = if date == "latest" {
        String::new()
    } else {
        date
    };
    let best = entries
        .iter()
        .filter(|entry| {
            entry
                .release
                .as_ref()
                .is_some_and(|(f, d)| f == family && d.starts_with(&date))
        })
        .max_by(|a, b| {
            a.release
                .cmp(&b.release)
                .then_with(|| a.source_release_date.cmp(&b.source_release_date))
                .then_with(|| a.modified.cmp(&b.modified))
                .then_with(|| b.name.cmp(&a.name))
        });
    Ok(best.map(|entry| entry.path.clone()))
}

/// Resolves what a user typed as an index: an existing path, else a library
/// reference. The error lists what the library holds.
pub fn resolve(reference: &str) -> Result<PathBuf> {
    let path = Path::new(reference);
    if path.exists() {
        return Ok(path.to_path_buf());
    }
    if let Some(found) = find(reference)? {
        return Ok(found);
    }
    let names: Vec<_> = entries().into_iter().map(|entry| entry.name).collect();
    if names.is_empty() {
        bail!(
            "No index or path named {reference}. The library is empty; add a release with \
             `snomed-ecl-engine add ARCHIVE`"
        );
    }
    bail!(
        "No index or path named {reference}. The library holds: {}",
        names.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "import")]
    #[test]
    fn local_add_prefers_exactly_one_known_edition_candidate() {
        use snomed_ecl_engine::import::ArchiveSummary;
        let map = "http://snomed.info/sct/2000002/version/20260801";
        let other = "http://snomed.info/sct/2000003/version/20260801";
        let uk = "http://snomed.info/sct/83821000000107/version/20260801";
        let international = "http://snomed.info/sct/900000000000207008/version/20260801";
        let mut summary = ArchiveSummary {
            sha256: String::new(),
            bytes: 0,
            effective_time: "20260801".into(),
            edition_uris: vec![],
            root_editions: 1,
            required_files: vec![],
            duplicate_files: vec![],
            importable: true,
        };
        for known in [uk, international] {
            summary.edition_uris = vec![map.into(), known.into()];
            // The known edition need not be a dependency root.
            assert_eq!(choose_local_edition(&summary).unwrap(), known);
        }
        for candidates in [vec![map, other], vec![map, uk, international], vec![]] {
            summary.edition_uris = candidates.iter().map(|uri| (*uri).into()).collect();
            let error = choose_local_edition(&summary).unwrap_err().to_string();
            assert!(error.contains("--edition URI"), "{error}");
            for uri in candidates {
                assert!(error.contains(uri), "{error}");
            }
        }
        summary.edition_uris = vec![other.into()];
        assert_eq!(choose_local_edition(&summary).unwrap(), other);
    }

    #[test]
    fn editions_have_short_names_and_dates() {
        let uk = "http://snomed.info/sct/83821000000107/version/20260826";
        assert_eq!(edition_parts(uk), Some(("uk".into(), "20260826".into())));
        assert_eq!(default_name(uk).as_deref(), Some("uk-20260826"));
        let international = "http://snomed.info/sct/900000000000207008/version/20260801";
        assert_eq!(default_name(international).as_deref(), Some("int-20260801"));
        let other = "http://snomed.info/sct/11000146104/version/20260331";
        assert_eq!(default_name(other).as_deref(), Some("11000146104-20260331"));
        assert_eq!(edition_parts("http://snomed.info/sct/1/version/2026"), None);
        assert_eq!(edition_parts("not a uri"), None);
        assert_eq!(show_date("20260826"), "2026-08-26");
    }

    fn write_index(home: &Path, name: &str, source_date: Option<&str>, modified: u64) -> PathBuf {
        use sha2::{Digest, Sha256};
        let staging = tempfile::tempdir().unwrap();
        let core = b"synthetic core";
        std::fs::write(staging.path().join("core.bin"), core).unwrap();
        let source = source_date.map(|date| {
            serde_json::json!({
                "distributor": "trud", "item": 1799, "release_id": "synthetic_20260923.zip",
                "release_name": "Release 3.0.0", "release_date": date,
                "archive_file_name": "synthetic_20260923.zip",
            })
        });
        let manifest = serde_json::json!({
            "format": 1, "edition": "http://snomed.info/sct/83821000000107/version/20260923",
            "archive_sha256": "a".repeat(64), "source": source,
            "concept_count": 0, "active_concept_count": 0, "hierarchy_edges": 0,
            "attributes": 0, "concrete_attributes": 0, "concrete_values": 0,
            "core_bytes": core.len(), "core_sha256": format!("{:x}", Sha256::digest(core)),
            "display_bytes": 0, "display_sha256": "", "display_refsets": [],
            "displays_selected": 0, "module_dependencies": [], "capabilities": [],
        });
        std::fs::write(staging.path().join("manifest.json"), manifest.to_string()).unwrap();
        let path = home.join(format!("{name}.ecl"));
        snomed_ecl_engine::store::pack(staging.path(), &path).unwrap();
        let times = std::fs::FileTimes::new()
            .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(modified));
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(times)
            .unwrap();
        path
    }

    #[test]
    fn later_source_release_date_selects_the_reissue() {
        let home = tempfile::tempdir().unwrap();
        let original = write_index(home.path(), "uk-20260923", Some("2026-09-30"), 200);
        assert_eq!(
            find_among("uk", &entries_in(home.path())).unwrap(),
            Some(original.clone())
        );
        let reissue = write_index(home.path(), "uk-20260923-9f8e7d6c", Some("2026-10-06"), 100);
        let entries = entries_in(home.path());
        for reference in ["uk", "uk@latest", "uk@2026-09"] {
            assert_eq!(
                find_among(reference, &entries).unwrap(),
                Some(reissue.clone())
            );
        }
        assert_eq!(find_among("uk-20260923", &entries).unwrap(), Some(original));
    }

    #[test]
    fn later_mtime_selects_the_reissue_when_source_dates_tie_or_are_absent() {
        for source_date in [None, Some("2026-10-06")] {
            let home = tempfile::tempdir().unwrap();
            let original = write_index(home.path(), "uk-20260923", source_date, 100);
            let reissue = write_index(home.path(), "uk-20260923-9f8e7d6c", source_date, 200);
            let mut entries = entries_in(home.path());
            for reference in ["uk", "uk@latest", "uk@2026-09"] {
                assert_eq!(
                    find_among(reference, &entries).unwrap(),
                    Some(reissue.clone())
                );
            }
            // If both dates and modification times tie, retain the smaller name.
            for entry in &mut entries {
                entry.modified = Some(SystemTime::UNIX_EPOCH);
            }
            assert_eq!(find_among("uk", &entries).unwrap(), Some(original));
            // Edition date still takes precedence over a distributor's issue date.
            entries[0].release = Some(("uk".into(), "20261001".into()));
            assert_eq!(
                find_among("uk", &entries).unwrap(),
                Some(entries[0].path.clone())
            );
        }
    }

    #[test]
    fn names_stay_portable() {
        assert!(check_name("uk-20260826").is_ok());
        assert!(check_name("uk_pcd.2026").is_ok());
        for bad in [
            "",
            ".hidden",
            "-flag",
            "a/b",
            "a b",
            "a\\b",
            &"x".repeat(65),
        ] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn international_index_resolves_by_short_name() {
        let edition = "http://snomed.info/sct/900000000000207008/version/20260801";
        let name = default_name(edition).unwrap();
        assert_eq!(name, "int-20260801");
        let path = PathBuf::from(format!("indexes/{name}.ecl"));
        let entries = [Entry {
            name,
            path: path.clone(),
            release: edition_parts(edition),
            source_release_date: None,
            modified: None,
        }];
        for reference in ["int", "int-20260801", "int@2026-08"] {
            assert_eq!(find_among(reference, &entries).unwrap(), Some(path.clone()));
        }
    }
}
