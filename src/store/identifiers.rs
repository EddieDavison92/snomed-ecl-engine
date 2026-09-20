use super::{sha256, IndexSource, Section};
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::OnceLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identifier {
    pub scheme: u64,
    pub code: String,
    pub referenced_component: u64,
    pub module: u64,
    pub effective_time: u32,
    pub active: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IdentifierIndex {
    pub rows: Vec<Identifier>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentifierManifest {
    pub bytes: u64,
    pub sha256: String,
    pub rows: usize,
}
impl IdentifierIndex {
    pub fn build(mut rows: Vec<Identifier>) -> Result<Self> {
        rows.sort_unstable_by(|a, b| (a.scheme, &a.code).cmp(&(b.scheme, &b.code)));
        let result = Self { rows };
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.rows
                .windows(2)
                .all(|w| (w[0].scheme, &w[0].code) < (w[1].scheme, &w[1].code)),
            "Duplicate or unsorted alternate identifiers"
        );
        for row in &self.rows {
            ensure!(
                !row.code.is_empty() && !row.code.chars().any(|c| c.is_ascii_control()),
                "Invalid alternate identifier code"
            );
            ensure!(
                [row.scheme, row.module, row.referenced_component]
                    .iter()
                    .all(|id| (100000..1_000_000_000_000_000_000).contains(id)),
                "Invalid identifier SCTID"
            );
            ensure!(
                super::members::valid_time(row.effective_time),
                "Invalid identifier effective time"
            );
        }
        Ok(())
    }
    pub fn lookup(&self, scheme: u64, code: &str) -> Option<u64> {
        let pos = self
            .rows
            .binary_search_by(|r| (r.scheme, r.code.as_str()).cmp(&(scheme, code)))
            .ok()?;
        let row = &self.rows[pos];
        (row.active && matches!((row.referenced_component / 10) % 100, 0 | 10))
            .then_some(row.referenced_component)
    }
    pub fn write(&self, directory: &Path) -> Result<IdentifierManifest> {
        self.validate()?;
        let path = directory.join("identifiers.json");
        let mut file = std::fs::File::create_new(&path)?;
        serde_json::to_writer(&mut file, self)?;
        file.sync_all()?;
        Ok(IdentifierManifest {
            bytes: path.metadata()?.len(),
            sha256: sha256(&path)?,
            rows: self.rows.len(),
        })
    }
    pub(super) fn open(section: &Section, manifest: &IdentifierManifest) -> Result<Self> {
        section.verify()?;
        let index: Self = serde_json::from_reader(std::io::BufReader::new(section.reader()?))?;
        index.validate()?;
        ensure!(
            index.rows.len() == manifest.rows,
            "Identifier count differs from manifest"
        );
        Ok(index)
    }
}
#[derive(Debug, Default)]
pub struct IdentifierStore {
    source: Option<(Section, IdentifierManifest)>,
    loaded: OnceLock<std::result::Result<IdentifierIndex, String>>,
}
impl IdentifierStore {
    pub fn loaded(index: IdentifierIndex) -> Result<Self> {
        index.validate()?;
        Ok(Self {
            source: None,
            loaded: OnceLock::from(Ok(index)),
        })
    }
    pub(super) fn lazy(source: &IndexSource, manifest: IdentifierManifest) -> Result<Self> {
        Ok(Self {
            source: Some((source.section("identifiers.json")?, manifest)),
            loaded: OnceLock::new(),
        })
    }
    pub fn get(&self) -> Result<Option<&IdentifierIndex>> {
        if self.source.is_none() && self.loaded.get().is_none() {
            return Ok(None);
        }
        match self.loaded.get_or_init(|| {
            let (path, manifest) = self.source.as_ref().unwrap();
            IdentifierIndex::open(path, manifest).map_err(|e| e.to_string())
        }) {
            Ok(index) => Ok(Some(index)),
            Err(e) => bail!("Identifier index: {e}"),
        }
    }
}
