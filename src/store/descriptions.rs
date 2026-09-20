use super::*;
use std::sync::OnceLock;

const MAGIC: &[u8; 8] = b"SNDES001";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DescriptionManifest {
    pub bytes: u64,
    pub sha256: String,
    pub descriptions: usize,
    pub active_descriptions: usize,
    pub language_memberships: usize,
}

/// Input to the offline builder. Terms are packed into one UTF-8 buffer on build.
#[derive(Debug)]
pub struct Description {
    pub id: u64,
    pub concept: u32,
    pub module: u32,
    pub kind: u32,
    pub effective_time: u32,
    pub active: bool,
    pub language: [u8; 2],
    pub term: String,
    /// Active language members: (refset ordinal, acceptability ordinal).
    pub dialects: Vec<(u32, u32)>,
}

#[derive(Debug, Default)]
pub struct DescriptionIndex {
    pub(super) concepts: Vec<u32>,
    ids: Vec<u64>,
    modules: Vec<u32>,
    kinds: Vec<u32>,
    dates: Vec<u32>,
    flags: Vec<u32>,
    term_offsets: Vec<u32>,
    terms: String,
    dialect_offsets: Vec<u32>,
    dialects: Vec<u32>,
}

impl DescriptionIndex {
    pub fn build(count: usize, mut descriptions: Vec<Description>) -> Result<Self> {
        descriptions.sort_unstable_by_key(|d| (d.concept, d.id));
        let mut index = Self {
            concepts: vec![0; count + 1],
            term_offsets: vec![0],
            dialect_offsets: vec![0],
            ..Self::default()
        };
        let mut ids = std::collections::HashSet::new();
        for mut d in descriptions {
            ensure!(ids.insert(d.id), "Duplicate description ID");
            ensure!((d.concept as usize) < count, "Unknown description concept");
            index.concepts[d.concept as usize + 1] += 1;
            index.ids.push(d.id);
            index.modules.push(d.module);
            index.kinds.push(d.kind);
            index.dates.push(d.effective_time);
            index
                .flags
                .push(u16::from_le_bytes(d.language) as u32 | (u32::from(d.active) << 16));
            index.terms.push_str(&d.term);
            index.term_offsets.push(u32::try_from(index.terms.len())?);
            d.dialects.sort_unstable();
            d.dialects.dedup();
            for (refset, acceptability) in d.dialects {
                index.dialects.extend([refset, acceptability]);
            }
            index
                .dialect_offsets
                .push(u32::try_from(index.dialects.len())?);
        }
        prefix_sum(&mut index.concepts)?;
        index.validate(count)?;
        Ok(index)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
    pub fn for_concept(&self, concept: u32) -> std::ops::Range<usize> {
        self.concepts[concept as usize] as usize..self.concepts[concept as usize + 1] as usize
    }
    pub fn id(&self, row: usize) -> u64 {
        self.ids[row]
    }
    pub fn module(&self, row: usize) -> u32 {
        self.modules[row]
    }
    pub fn kind(&self, row: usize) -> u32 {
        self.kinds[row]
    }
    pub fn effective_time(&self, row: usize) -> u32 {
        self.dates[row]
    }
    pub fn active(&self, row: usize) -> bool {
        self.flags[row] & (1 << 16) != 0
    }
    pub fn language(&self, row: usize) -> [u8; 2] {
        (self.flags[row] as u16).to_le_bytes()
    }
    pub fn term(&self, row: usize) -> &str {
        &self.terms[self.term_offsets[row] as usize..self.term_offsets[row + 1] as usize]
    }
    pub fn dialects(&self, row: usize) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.dialects[self.dialect_offsets[row] as usize..self.dialect_offsets[row + 1] as usize]
            .chunks_exact(2)
            .map(|pair| (pair[0], pair[1]))
    }

    fn validate(&self, count: usize) -> Result<()> {
        let n = self.len();
        validate_offsets(&self.concepts, count, n)?;
        validate_offsets(&self.term_offsets, n, self.terms.len())?;
        validate_offsets(&self.dialect_offsets, n, self.dialects.len())?;
        ensure!(
            [
                self.modules.len(),
                self.kinds.len(),
                self.dates.len(),
                self.flags.len()
            ]
            .iter()
            .all(|&v| v == n),
            "Description column length mismatch"
        );
        ensure!(
            self.modules
                .iter()
                .chain(&self.kinds)
                .chain(&self.dialects)
                .all(|&v| (v as usize) < count),
            "Invalid description metadata ordinal"
        );
        ensure!(
            self.term_offsets
                .iter()
                .all(|&v| self.terms.is_char_boundary(v as usize)),
            "Invalid description UTF-8 offset"
        );
        ensure!(
            self.dialect_offsets.iter().all(|v| v % 2 == 0),
            "Invalid language member offset"
        );
        let mut ids = self.ids.clone();
        ids.sort_unstable();
        ensure!(
            ids.iter()
                .all(|v| (100_000..1_000_000_000_000_000_000).contains(v))
                && ids.windows(2).all(|w| w[0] < w[1]),
            "Invalid or duplicate description ID"
        );
        for row in 0..n {
            ensure!(
                self.flags[row] >> 17 == 0 && self.language(row).iter().all(u8::is_ascii_lowercase),
                "Invalid description language or flags"
            );
            ensure!(!self.term(row).is_empty(), "Empty description term");
            let mut previous = None;
            for pair in self.dialects(row) {
                ensure!(
                    previous.is_none_or(|old| old < pair),
                    "Unordered language members"
                );
                previous = Some(pair);
            }
        }
        Ok(())
    }

    pub fn write(&self, path: &Path) -> Result<DescriptionManifest> {
        self.validate(self.concepts.len().saturating_sub(1))?;
        let mut out = BufWriter::new(File::create_new(path)?);
        out.write_all(MAGIC)?;
        put_u32s(&mut out, &self.concepts)?;
        put_u64(&mut out, self.ids.len() as u64)?;
        for &id in &self.ids {
            put_u64(&mut out, id)?;
        }
        for column in [
            &self.modules,
            &self.kinds,
            &self.dates,
            &self.flags,
            &self.term_offsets,
            &self.dialect_offsets,
            &self.dialects,
        ] {
            put_u32s(&mut out, column)?;
        }
        put_u64(&mut out, self.terms.len() as u64)?;
        out.write_all(self.terms.as_bytes())?;
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(DescriptionManifest {
            bytes: path.metadata()?.len(),
            sha256: sha256(path)?,
            descriptions: self.len(),
            active_descriptions: (0..self.len()).filter(|&i| self.active(i)).count(),
            language_memberships: self.dialects.len() / 2,
        })
    }

    pub(super) fn open(
        section: &Section,
        metadata: &DescriptionManifest,
        count: usize,
    ) -> Result<Self> {
        let mut input = Input::open(section, MAGIC)?;
        let concepts = input.u32s()?;
        let n = input.count(8)?;
        let index = Self {
            concepts,
            ids: (0..n).map(|_| input.u64()).collect::<Result<_>>()?,
            modules: input.u32s()?,
            kinds: input.u32s()?,
            dates: input.u32s()?,
            flags: input.u32s()?,
            term_offsets: input.u32s()?,
            dialect_offsets: input.u32s()?,
            dialects: input.u32s()?,
            terms: String::from_utf8(input.bytes()?)?,
        };
        ensure!(input.remaining == 0, "Trailing description bytes");
        index.validate(count)?;
        ensure!(
            index.len() == metadata.descriptions
                && index.dialects.len() / 2 == metadata.language_memberships
                && (0..index.len()).filter(|&r| index.active(r)).count()
                    == metadata.active_descriptions,
            "Description manifest counts differ"
        );
        Ok(index)
    }

    #[cfg(feature = "import")]
    pub(crate) fn into_descriptions(self, mapping: &[u32]) -> Vec<Description> {
        let mut rows = Vec::with_capacity(self.len());
        for (concept, &new) in mapping.iter().enumerate() {
            for row in self.for_concept(concept as u32) {
                rows.push(Description {
                    id: self.id(row),
                    concept: new,
                    module: mapping[self.module(row) as usize],
                    kind: mapping[self.kind(row) as usize],
                    effective_time: self.effective_time(row),
                    active: self.active(row),
                    language: self.language(row),
                    term: self.term(row).to_owned(),
                    dialects: self
                        .dialects(row)
                        .map(|(r, a)| (mapping[r as usize], mapping[a as usize]))
                        .collect(),
                });
            }
        }
        rows
    }
}

/// The sidecar is opened only when requested. Numeric queries perform no text I/O.
#[derive(Debug, Default)]
pub struct DescriptionStore {
    source: Option<(Section, DescriptionManifest, usize)>,
    loaded: OnceLock<std::result::Result<DescriptionIndex, String>>,
}
impl DescriptionStore {
    pub fn loaded(index: DescriptionIndex) -> Self {
        Self {
            source: None,
            loaded: OnceLock::from(Ok(index)),
        }
    }
    pub(super) fn lazy(
        source: &IndexSource,
        metadata: DescriptionManifest,
        count: usize,
    ) -> Result<Self> {
        Ok(Self {
            source: Some((source.section("descriptions.bin")?, metadata, count)),
            loaded: OnceLock::new(),
        })
    }
    pub fn get(&self) -> Result<Option<&DescriptionIndex>> {
        if self.source.is_none() && self.loaded.get().is_none() {
            return Ok(None);
        }
        match self.loaded.get_or_init(|| {
            let (path, metadata, count) = self.source.as_ref().unwrap();
            DescriptionIndex::open(path, metadata, *count).map_err(|e| e.to_string())
        }) {
            Ok(index) => Ok(Some(index)),
            Err(message) => bail!("Description index: {message}"),
        }
    }
    #[cfg(feature = "import")]
    pub(crate) fn into_index(mut self) -> Result<Option<DescriptionIndex>> {
        self.get()?;
        self.loaded
            .take()
            .map(|r| r.map_err(anyhow::Error::msg))
            .transpose()
    }
}
