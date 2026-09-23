//! The index library: packed indexes kept in one folder, named by edition and
//! release date, so they can be chosen as `uk@2026-08` rather than by path.
//!
//! Like the selection in `workspace`, this belongs to the CLI. The library
//! never changes what an index answers.
use anyhow::{bail, Context, Result};
use snomed_ecl_engine::store::Manifest;
use std::path::{Path, PathBuf};

pub const ENV_HOME: &str = "SNOMED_ECL_HOME";

/// Editions with a short name. Any other edition is named by its module ID.
const FAMILIES: &[(&str, &str)] = &[("83821000000107", "uk"), ("900000000000207008", "int")];

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
    let family = FAMILIES
        .iter()
        .find(|(id, _)| *id == module)
        .map_or_else(|| module.to_owned(), |(_, name)| (*name).to_owned());
    Some((family, date.to_owned()))
}

/// The name an index gets when none is given, such as `uk-20260826`.
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
}

/// Every index in the library, by name.
pub fn entries() -> Vec<Entry> {
    let Ok(home) = home() else {
        return Vec::new();
    };
    let Ok(listing) = std::fs::read_dir(&home) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = listing
        .flatten()
        .map(|item| item.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ecl") && path.is_file())
        .filter_map(|path| {
            let name = path.file_stem()?.to_str()?.to_owned();
            let release = edition_parts(&Manifest::read(&path).ok()?.edition);
            Some(Entry {
                name,
                path,
                release,
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
    let entries = entries();
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
        .max_by(|a, b| a.release.cmp(&b.release).then_with(|| b.name.cmp(&a.name)));
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

    #[test]
    fn editions_have_short_names_and_dates() {
        let uk = "http://snomed.info/sct/83821000000107/version/20260826";
        assert_eq!(edition_parts(uk), Some(("uk".into(), "20260826".into())));
        assert_eq!(default_name(uk).as_deref(), Some("uk-20260826"));
        let other = "http://snomed.info/sct/11000146104/version/20260331";
        assert_eq!(default_name(other).as_deref(), Some("11000146104-20260331"));
        assert_eq!(edition_parts("http://snomed.info/sct/1/version/2026"), None);
        assert_eq!(edition_parts("not a uri"), None);
        assert_eq!(show_date("20260826"), "2026-08-26");
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
}
