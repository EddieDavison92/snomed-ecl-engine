use super::columns::{Column, RowSets};
use super::term_storage::TermStorage;
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
    modules: Column,
    kinds: Column,
    dates: Column,
    flags: Column,
    term_offsets: Vec<u32>,
    terms: TermStorage,
    dialects: RowSets,
}

impl DescriptionIndex {
    pub fn build(count: usize, mut descriptions: Vec<Description>) -> Result<Self> {
        descriptions.sort_unstable_by_key(|d| (d.concept, d.id));
        let mut index = Self {
            concepts: vec![0; count + 1],
            term_offsets: vec![0],
            ..Self::default()
        };
        let (mut modules, mut kinds, mut dates, mut flags) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut dialect_offsets, mut dialects, mut terms) = (vec![0], Vec::new(), String::new());
        let mut ids = std::collections::HashSet::new();
        for mut d in descriptions {
            ensure!(ids.insert(d.id), "Duplicate description ID");
            ensure!((d.concept as usize) < count, "Unknown description concept");
            index.concepts[d.concept as usize + 1] += 1;
            index.ids.push(d.id);
            modules.push(d.module);
            kinds.push(d.kind);
            dates.push(d.effective_time);
            flags.push(u16::from_le_bytes(d.language) as u32 | (u32::from(d.active) << 16));
            terms.push_str(&d.term);
            index.term_offsets.push(u32::try_from(terms.len())?);
            d.dialects.sort_unstable();
            d.dialects.dedup();
            for (refset, acceptability) in d.dialects {
                dialects.extend([refset, acceptability]);
            }
            dialect_offsets.push(u32::try_from(dialects.len())?);
        }
        index.modules = Column::new(modules);
        index.kinds = Column::new(kinds);
        index.dates = Column::new(dates);
        index.flags = Column::new(flags);
        index.dialects = RowSets::new(dialect_offsets, dialects, index.len())?;
        index.terms = TermStorage::Owned(terms);
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
        self.modules.get(row)
    }
    pub fn kind(&self, row: usize) -> u32 {
        self.kinds.get(row)
    }
    pub fn effective_time(&self, row: usize) -> u32 {
        self.dates.get(row)
    }
    pub fn active(&self, row: usize) -> bool {
        self.flags.get(row) & (1 << 16) != 0
    }
    pub fn language(&self, row: usize) -> [u8; 2] {
        (self.flags.get(row) as u16).to_le_bytes()
    }
    pub fn term(&self, row: usize) -> Result<String> {
        self.with_term(row, str::to_owned)
    }
    pub fn term_bytes(&self, row: usize) -> usize {
        (self.term_offsets[row + 1] - self.term_offsets[row]) as usize
    }
    /// Visits text without allocating a string. The callback must not re-enter this index's text reader.
    pub fn with_term<T>(&self, row: usize, visit: impl FnOnce(&str) -> T) -> Result<T> {
        self.terms.with_range(
            self.term_offsets[row] as usize,
            self.term_offsets[row + 1] as usize,
            visit,
        )
    }
    pub fn dialects(&self, row: usize) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.dialects
            .get(row)
            .chunks_exact(2)
            .map(|pair| (pair[0], pair[1]))
    }

    fn validate(&self, count: usize) -> Result<()> {
        let n = self.len();
        validate_offsets(&self.concepts, count, n)?;
        validate_offsets(&self.term_offsets, n, self.terms.len())?;
        self.dialects.validate(count, n)?;
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
                .chain(self.kinds.iter())
                .all(|v| (v as usize) < count),
            "Invalid description metadata ordinal"
        );
        self.terms.validate(&self.term_offsets)?;
        ensure!(
            self.term_offsets.windows(2).all(|w| w[0] < w[1]),
            "Empty description term"
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
                self.flags.get(row) >> 17 == 0
                    && self.language(row).iter().all(u8::is_ascii_lowercase),
                "Invalid description language or flags"
            );
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
        for column in [&self.modules, &self.kinds, &self.dates, &self.flags] {
            column.write(&mut out)?;
        }
        put_u32s(&mut out, &self.term_offsets)?;
        self.dialects.write(&mut out)?;
        self.terms.write(&mut out)?;
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(DescriptionManifest {
            bytes: path.metadata()?.len(),
            sha256: sha256(path)?,
            descriptions: self.len(),
            active_descriptions: (0..self.len()).filter(|&i| self.active(i)).count(),
            language_memberships: self.dialects.entries() / 2,
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
        let ids = (0..n).map(|_| input.u64()).collect::<Result<_>>()?;
        let modules = Column::new(input.u32s()?);
        let kinds = Column::new(input.u32s()?);
        let dates = Column::new(input.u32s()?);
        let flags = Column::new(input.u32s()?);
        let term_offsets = input.u32s()?;
        let dialects = RowSets::read(&mut input, n)?;
        let length = input.count(1)?;
        ensure!(
            input.remaining == length as u64,
            "Trailing description bytes"
        );
        let start = input.reader.stream_position()?;
        let index = Self {
            concepts,
            ids,
            modules,
            kinds,
            dates,
            flags,
            term_offsets,
            dialects,
            terms: TermStorage::stored(section.clone(), start, length)?,
        };
        index.validate(count)?;
        ensure!(
            index.len() == metadata.descriptions
                && index.dialects.entries() / 2 == metadata.language_memberships
                && (0..index.len()).filter(|&r| index.active(r)).count()
                    == metadata.active_descriptions,
            "Description manifest counts differ"
        );
        Ok(index)
    }

    #[cfg(feature = "import")]
    pub(crate) fn into_descriptions(self, mapping: &[u32]) -> Result<Vec<Description>> {
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
                    term: self.term(row)?,
                    dialects: self
                        .dialects(row)
                        .map(|(r, a)| (mapping[r as usize], mapping[a as usize]))
                        .collect(),
                });
            }
        }
        Ok(rows)
    }
}

/// One description, as read for a single concept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptionRow {
    pub id: u64,
    pub module: u32,
    pub kind: u32,
    pub effective_time: u32,
    pub active: bool,
    pub language: [u8; 2],
    pub term: String,
    /// Active language members: (refset ordinal, acceptability ordinal).
    pub dialects: Vec<(u32, u32)>,
}

/// Where each array of the description section starts, so one concept's rows
/// can be read without loading the rest. Every array is a u64 length followed
/// by fixed-width values; see `DescriptionIndex::write`.
#[derive(Debug)]
struct Layout {
    count: usize,
    rows: u64,
    concepts: u64,
    ids: u64,
    /// Modules, kinds, dates and flags, in that order.
    columns: [u64; 4],
    term_offsets: u64,
    dialect_offsets: u64,
    dialect_values: u64,
    dialect_entries: u64,
    terms: u64,
    term_bytes: u64,
}

/// Reads a section at positions: without a lock when it is uncompressed,
/// otherwise through one shared block reader.
#[derive(Debug)]
struct Seeker {
    positional: Option<super::container::PositionalReader>,
    reader: std::sync::Mutex<SectionReader>,
    layout: Layout,
}
impl Seeker {
    fn open(section: &Section, count: usize) -> Result<Self> {
        let mut seeker = Self {
            positional: section.positional()?,
            reader: std::sync::Mutex::new(section.reader()?),
            layout: Layout {
                count,
                rows: 0,
                concepts: 0,
                ids: 0,
                columns: [0; 4],
                term_offsets: 0,
                dialect_offsets: 0,
                dialect_values: 0,
                dialect_entries: 0,
                terms: 0,
                term_bytes: 0,
            },
        };
        let mut magic = [0; 8];
        seeker.read(0, &mut magic)?;
        ensure!(&magic == MAGIC, "Invalid description section");
        // Walks the length prefixes, checking each array fits before the next.
        let mut at = 8u64;
        let mut array = |seeker: &Self, width: u64, expected: Option<u64>| -> Result<(u64, u64)> {
            let length = seeker.u64(at)?;
            ensure!(
                expected.is_none_or(|e| e == length),
                "Description array length differs"
            );
            let start = at + 8;
            at = length
                .checked_mul(width)
                .and_then(|bytes| start.checked_add(bytes))
                .filter(|&end| end <= section.length)
                .context("Description array exceeds its section")?;
            Ok((start, length))
        };
        let (concepts, _) = array(&seeker, 4, Some(count as u64 + 1))?;
        let (ids, rows) = array(&seeker, 8, None)?;
        let mut columns = [0; 4];
        for column in &mut columns {
            *column = array(&seeker, 4, Some(rows))?.0;
        }
        let (term_offsets, _) = array(&seeker, 4, Some(rows + 1))?;
        let (dialect_offsets, _) = array(&seeker, 4, Some(rows + 1))?;
        let (dialect_values, dialect_entries) = array(&seeker, 4, None)?;
        let (terms, term_bytes) = array(&seeker, 1, None)?;
        ensure!(at == section.length, "Trailing description bytes");
        seeker.layout = Layout {
            count,
            rows,
            concepts,
            ids,
            columns,
            term_offsets,
            dialect_offsets,
            dialect_values,
            dialect_entries,
            terms,
            term_bytes,
        };
        Ok(seeker)
    }
    fn read(&self, position: u64, bytes: &mut [u8]) -> Result<()> {
        if let Some(reader) = &self.positional {
            reader.read_exact_at(position, bytes)?;
        } else {
            let mut reader = self
                .reader
                .lock()
                .map_err(|_| anyhow::anyhow!("Description reader poisoned"))?;
            reader.seek(SeekFrom::Start(position))?;
            reader.read_exact(bytes)?;
        }
        Ok(())
    }
    fn u64(&self, position: u64) -> Result<u64> {
        let mut bytes = [0; 8];
        self.read(position, &mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }
    /// `count` consecutive u32 values of the array at `start`, from `first`.
    fn u32s(&self, start: u64, first: u64, count: u64) -> Result<Vec<u32>> {
        let mut bytes = vec![0; usize::try_from(count * 4)?];
        self.read(start + first * 4, &mut bytes)?;
        Ok(bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect())
    }
    fn rows(&self, concept: u32) -> Result<Vec<DescriptionRow>> {
        let layout = &self.layout;
        ensure!((concept as usize) < layout.count, "Description concept out of range");
        let bounds = self.u32s(layout.concepts, concept as u64, 2)?;
        let (first, last) = (bounds[0] as u64, bounds[1] as u64);
        ensure!(first <= last && last <= layout.rows, "Invalid description offsets");
        let k = last - first;
        if k == 0 {
            return Ok(Vec::new());
        }
        let mut ids = vec![0; usize::try_from(k * 8)?];
        self.read(layout.ids + first * 8, &mut ids)?;
        let [modules, kinds, dates, flags] = layout.columns.map(|at| self.u32s(at, first, k));
        let (modules, kinds, dates, flags) = (modules?, kinds?, dates?, flags?);
        let terms = self.u32s(layout.term_offsets, first, k + 1)?;
        let dialects = self.u32s(layout.dialect_offsets, first, k + 1)?;
        let (term_start, term_end) = (terms[0] as u64, terms[k as usize] as u64);
        let (dialect_start, dialect_end) = (dialects[0] as u64, dialects[k as usize] as u64);
        ensure!(
            terms.windows(2).all(|w| w[0] < w[1])
                && term_end <= layout.term_bytes
                && dialects.windows(2).all(|w| w[0] <= w[1])
                && dialects.iter().all(|v| v % 2 == 0)
                && dialect_end <= layout.dialect_entries,
            "Invalid description text or language offsets"
        );
        let mut text = vec![0; usize::try_from(term_end - term_start)?];
        self.read(layout.terms + term_start, &mut text)?;
        let members = self.u32s(layout.dialect_values, dialect_start, dialect_end - dialect_start)?;
        let count = layout.count as u32;
        let mut rows = Vec::with_capacity(k as usize);
        for i in 0..k as usize {
            let id = u64::from_le_bytes(ids[i * 8..i * 8 + 8].try_into().unwrap());
            let language = (flags[i] as u16).to_le_bytes();
            ensure!(
                modules[i] < count
                    && kinds[i] < count
                    && flags[i] >> 17 == 0
                    && language.iter().all(u8::is_ascii_lowercase)
                    && (100_000..1_000_000_000_000_000_000).contains(&id),
                "Invalid description row"
            );
            let term = &text[(terms[i] as u64 - term_start) as usize..(terms[i + 1] as u64 - term_start) as usize];
            let pairs = &members[(dialects[i] as u64 - dialect_start) as usize
                ..(dialects[i + 1] as u64 - dialect_start) as usize];
            ensure!(
                pairs.iter().all(|&v| v < count),
                "Invalid description dialect ordinal"
            );
            rows.push(DescriptionRow {
                id,
                module: modules[i],
                kind: kinds[i],
                effective_time: dates[i],
                active: flags[i] & (1 << 16) != 0,
                language,
                term: std::str::from_utf8(term)
                    .context("Invalid description UTF-8")?
                    .to_owned(),
                dialects: pairs.chunks_exact(2).map(|p| (p[0], p[1])).collect(),
            });
        }
        Ok(rows)
    }
}

/// The sidecar is opened only when requested. Numeric queries perform no text I/O.
#[derive(Debug, Default)]
pub struct DescriptionStore {
    source: Option<(Section, DescriptionManifest, usize)>,
    loaded: OnceLock<std::result::Result<DescriptionIndex, String>>,
    seeker: OnceLock<std::result::Result<Seeker, String>>,
}
impl DescriptionStore {
    pub fn loaded(index: DescriptionIndex) -> Self {
        Self {
            source: None,
            loaded: OnceLock::from(Ok(index)),
            seeker: OnceLock::new(),
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
            seeker: OnceLock::new(),
        })
    }
    /// Whether this store has descriptions at all.
    pub fn is_available(&self) -> bool {
        self.source.is_some() || self.loaded.get().is_some()
    }
    /// Whether the whole index is in memory already.
    pub fn is_loaded(&self) -> bool {
        matches!(self.loaded.get(), Some(Ok(_)))
    }
    /// One concept's descriptions. Reads only that concept's rows unless the
    /// whole index is already loaded, so describing a concept costs a few
    /// small reads rather than loading every description in the edition.
    pub fn concept_rows(&self, concept: u32) -> Result<Option<Vec<DescriptionRow>>> {
        if let Some(Ok(index)) = self.loaded.get() {
            return Ok(Some(
                index
                    .for_concept(concept)
                    .map(|row| {
                        Ok(DescriptionRow {
                            id: index.id(row),
                            module: index.module(row),
                            kind: index.kind(row),
                            effective_time: index.effective_time(row),
                            active: index.active(row),
                            language: index.language(row),
                            term: index.term(row)?,
                            dialects: index.dialects(row).collect(),
                        })
                    })
                    .collect::<Result<_>>()?,
            ));
        }
        let Some((section, _, count)) = &self.source else {
            return Ok(None);
        };
        match self
            .seeker
            .get_or_init(|| Seeker::open(section, *count).map_err(|e| e.to_string()))
        {
            Ok(seeker) => seeker.rows(concept).map(Some),
            Err(message) => bail!("Description index: {message}"),
        }
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
