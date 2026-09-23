//! Remember which index later commands use, and find indexes on disk.
//!
//! Selection state belongs to the CLI, not the library: it is one path in a
//! small JSON file outside the repository. Nothing here changes query results.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use snomed_ecl_engine::store::Manifest;
use std::path::{Path, PathBuf};

pub const ENV_STORE: &str = "SNOMED_ECL_STORE";

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    /// Absolute path to the selected index, when one has been selected.
    pub store: Option<PathBuf>,
}

/// Where the selected index is recorded, honouring the platform's config home.
pub fn state_path() -> Result<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    };
    let base = base.context(if cfg!(windows) {
        "Cannot locate APPDATA for the selected-index file"
    } else {
        "Cannot locate XDG_CONFIG_HOME or HOME for the selected-index file"
    })?;
    Ok(base.join("snomed-ecl-engine").join("state.json"))
}

/// Reads the recorded selection. An unreadable or malformed file means "none
/// selected" rather than a failure, so a stale file cannot block every command.
pub fn load() -> State {
    state_path()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(state: &State) -> Result<()> {
    let path = state_path()?;
    let directory = path.parent().context("Selected-index path has no parent")?;
    std::fs::create_dir_all(directory)
        .with_context(|| format!("Cannot create {}", directory.display()))?;
    std::fs::write(&path, serde_json::to_vec_pretty(state)?)
        .with_context(|| format!("Cannot write {}", path.display()))?;
    Ok(())
}

/// How the index in use was chosen, for messages that explain the choice.
#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    Argument,
    Environment,
    Selected,
}

impl Source {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Argument => "given on the command line",
            Self::Environment => "from SNOMED_ECL_STORE",
            Self::Selected => "selected earlier with `use`",
        }
    }
}

/// Resolves the index to query: the explicit argument, else the environment
/// variable, else the recorded selection. The first two may be a path or a
/// library reference such as `uk@2026-08`. The index is not opened here.
pub fn resolve(explicit: Option<&str>) -> Result<(PathBuf, Source)> {
    if let Some(reference) = explicit {
        return Ok((crate::library::resolve(reference)?, Source::Argument));
    }
    if let Some(reference) = std::env::var(ENV_STORE)
        .ok()
        .filter(|value| !value.is_empty())
    {
        return Ok((crate::library::resolve(&reference)?, Source::Environment));
    }
    if let Some(path) = load().store {
        return Ok((path, Source::Selected));
    }
    bail!(
        "No index selected. Give an index name or path, or select one first:\n\
         \x20 snomed-ecl-engine add ARCHIVE   build an index from an RF2 release\n\
         \x20 snomed-ecl-engine list          list the indexes available\n\
         \x20 snomed-ecl-engine use NAME      remember one for later commands\n\
         Setting {ENV_STORE} overrides the selection for one shell."
    )
}

/// An index found on disk, with the manifest facts worth showing in a list.
pub struct Found {
    /// The library name, for an index in the library folder.
    pub name: Option<String>,
    pub path: PathBuf,
    pub edition: String,
    pub active_concepts: usize,
    pub concepts: usize,
    pub bytes: u64,
    pub packed: bool,
    pub selected: bool,
}

/// Directories searched when `list` is given no path: the library, the working
/// directory and the conventional `data` folder beneath it.
pub fn default_roots() -> Vec<PathBuf> {
    let mut roots: Vec<_> = crate::library::home().into_iter().collect();
    roots.extend([PathBuf::from("."), PathBuf::from("data")]);
    roots
}

/// Lists indexes directly inside each root, and each root that is itself an
/// index. Entries that are not indexes are skipped silently; a root that does
/// not exist is not an error, because the default roots are conventions.
pub fn discover(roots: &[PathBuf], selected: Option<&Path>) -> Vec<Found> {
    let mut found = Vec::new();
    let mut seen = Vec::new();
    for root in roots {
        let mut candidates = vec![root.clone()];
        if let Ok(entries) = std::fs::read_dir(root) {
            candidates.extend(entries.flatten().map(|entry| entry.path()));
        }
        for candidate in candidates {
            let key = std::fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
            if seen.contains(&key) {
                continue;
            }
            seen.push(key.clone());
            if let Some(entry) = inspect(&candidate, selected) {
                found.push(entry);
            }
        }
    }
    // Library indexes first, by name; others by path.
    found.sort_by(|a, b| {
        (a.name.is_none(), &a.name, &a.path).cmp(&(b.name.is_none(), &b.name, &b.path))
    });
    found
}

/// Reads one candidate's manifest. Returns None for anything that is not an
/// index, which is the common case when scanning a working directory.
pub fn inspect(path: &Path, selected: Option<&Path>) -> Option<Found> {
    let packed = path.is_file();
    if packed {
        // Skip large non-index files rather than opening every archive found.
        let header_matches = std::fs::File::open(path)
            .and_then(|mut file| {
                use std::io::Read;
                let mut magic = [0; 8];
                file.read_exact(&mut magic)?;
                Ok(&magic == b"SNECL002")
            })
            .unwrap_or(false);
        if !header_matches {
            return None;
        }
    } else if !path.join("manifest.json").is_file() {
        return None;
    }
    let manifest = Manifest::read(path).ok()?;
    let canonical = std::fs::canonicalize(path).ok();
    let selected = match (selected, &canonical) {
        (Some(selected), Some(canonical)) => {
            std::fs::canonicalize(selected).is_ok_and(|selected| &selected == canonical)
        }
        _ => false,
    };
    let home = crate::library::home()
        .ok()
        .and_then(|home| std::fs::canonicalize(home).ok());
    let in_library = packed
        && path.extension().is_some_and(|ext| ext == "ecl")
        && canonical
            .as_deref()
            .and_then(Path::parent)
            .is_some_and(|parent| Some(parent) == home.as_deref());
    Some(Found {
        name: in_library
            .then(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_owned)
            })
            .flatten(),
        bytes: size_of_index(path, packed),
        edition: manifest.edition,
        active_concepts: manifest.active_concept_count,
        concepts: manifest.concept_count,
        path: path.to_path_buf(),
        packed,
        selected,
    })
}

/// Total bytes on disk: the file for a packed index, or the sum of one
/// directory level for an unpacked one. Nested directories are included.
fn size_of_index(path: &Path, packed: bool) -> u64 {
    if packed {
        return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(metadata) if metadata.is_dir() => size_of_index(&entry.path(), false),
            Ok(metadata) => metadata.len(),
            Err(_) => 0,
        })
        .sum()
}
