//! Historical association rows, indexed from both ends.
//!
//! A retired concept records what it became in one of several association
//! reference sets: SAME AS, REPLACED BY, POSSIBLY EQUIVALENT TO and so on. The
//! member tables hold those rows, but only in the order the release shipped
//! them, so asking "what did this code become" or "which retired codes pointed
//! here" meant scanning every row of every association table: about 430,000
//! rows and two binary searches per row, for every question.
//!
//! This keeps the same active rows twice, once keyed by the retired concept
//! and once keyed by the concept it points at. Each direction is a sorted key
//! list with the rows for each key stored contiguously, so a lookup is one
//! binary search and a slice.
use super::*;
use std::sync::OnceLock;

const MAGIC: &[u8; 8] = b"SNECLH01";
/// The parent of every historical association reference set.
pub const ASSOCIATIONS: u64 = 900000000000522004;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct HistoryManifest {
    pub bytes: u64,
    pub sha256: String,
    pub rows: usize,
    /// The association reference sets the rows came from.
    pub refsets: Vec<u64>,
    /// Active rows left out because one end is not a concept in this edition.
    #[serde(default)]
    pub skipped: usize,
}

/// One direction: keys, and for each key the concept at the other end of each
/// row together with the association that links them.
#[derive(Debug, Default)]
struct Keyed {
    keys: Vec<u32>,
    offsets: Vec<u32>,
    others: Vec<u32>,
    kinds: Vec<u8>,
}

impl Keyed {
    fn build(mut rows: Vec<(u32, u32, u8)>) -> Result<Self> {
        rows.sort_unstable();
        rows.dedup();
        let mut keyed = Self {
            offsets: vec![0],
            ..Self::default()
        };
        for (key, other, kind) in rows {
            if keyed.keys.last() != Some(&key) {
                keyed.keys.push(key);
                keyed.offsets.push(*keyed.offsets.last().expect("seeded"));
            }
            keyed.others.push(other);
            keyed.kinds.push(kind);
            *keyed.offsets.last_mut().expect("seeded") =
                u32::try_from(keyed.others.len()).context("History rows exceed u32")?;
        }
        Ok(keyed)
    }

    fn get(&self, key: u32) -> (&[u32], &[u8]) {
        match self.keys.binary_search(&key) {
            Ok(i) => {
                let range = self.offsets[i] as usize..self.offsets[i + 1] as usize;
                (&self.others[range.clone()], &self.kinds[range])
            }
            Err(_) => (&[], &[]),
        }
    }

    fn validate(&self, concepts: usize, kinds: usize) -> Result<()> {
        ensure!(
            self.offsets.len() == self.keys.len() + 1
                && self.offsets.first() == Some(&0)
                && self.offsets.last().map(|&v| v as usize) == Some(self.others.len())
                && self.others.len() == self.kinds.len(),
            "History index offsets do not cover their rows"
        );
        ensure!(
            self.offsets.windows(2).all(|w| w[0] < w[1]),
            "History index has an empty or unordered key"
        );
        ensure!(
            self.keys.windows(2).all(|w| w[0] < w[1]),
            "History keys are not sorted and unique"
        );
        ensure!(
            self.keys.last().is_none_or(|&k| (k as usize) < concepts)
                && self.others.iter().all(|&o| (o as usize) < concepts),
            "History index names a concept outside the store"
        );
        ensure!(
            self.kinds.iter().all(|&k| (k as usize) < kinds),
            "History index names an unknown association"
        );
        Ok(())
    }

    fn write(&self, out: &mut impl Write) -> Result<()> {
        put_u32s(out, &self.keys)?;
        put_u32s(out, &self.offsets)?;
        put_u32s(out, &self.others)?;
        put_u64(out, self.kinds.len() as u64)?;
        out.write_all(&self.kinds)?;
        Ok(())
    }

    fn read(input: &mut Input) -> Result<Self> {
        Ok(Self {
            keys: input.u32s()?,
            offsets: input.u32s()?,
            others: input.u32s()?,
            kinds: input.bytes()?,
        })
    }
}

/// A row as a caller sees it: the concept at the other end, and which
/// association links the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Association {
    pub concept: u32,
    pub refset: u64,
}

#[derive(Debug, Default)]
pub struct HistoryIndex {
    refsets: Vec<u64>,
    /// Keyed by the referenced component: this concept points at those.
    forward: Keyed,
    /// Keyed by the target: these concepts point at this one.
    backward: Keyed,
}

impl HistoryIndex {
    /// Builds from every active row of every association table in the store.
    ///
    /// A row whose referenced component or target is not a concept in this
    /// edition is left out: REFERS TO, for one, points descriptions at
    /// concepts, and neither lookup has a concept to key it by.
    pub fn build(store: &NumericStore) -> Result<(Self, usize)> {
        let associations: Vec<u64> = store
            .hierarchy(ASSOCIATIONS, false, false, false)
            .into_iter()
            .filter(|refset| store.member_tables.fields(*refset).is_some())
            .collect();
        let mut refsets = Vec::new();
        let mut rows = Vec::new();
        let mut skipped = 0;
        for refset in associations {
            let Some(table) = store.member_tables.get(refset)? else {
                continue;
            };
            let Some(MemberColumn::Id(targets)) = table.column("targetComponentId") else {
                continue;
            };
            let Some(MemberColumn::Id(references)) = table.column("referencedComponentId") else {
                bail!("Association table {refset} has no referenced components");
            };
            let Some(MemberColumn::Boolean(active)) = table.column("active") else {
                bail!("Association table {refset} has no active flags");
            };
            let kind = u8::try_from(refsets.len()).context("More than 255 associations")?;
            refsets.push(refset);
            for row in 0..table.len() {
                if active[row] == 0 {
                    continue;
                }
                match (store.ordinal(references[row]), store.ordinal(targets[row])) {
                    (Some(source), Some(target)) => rows.push((source, target, kind)),
                    _ => skipped += 1,
                }
            }
        }
        let backward = Keyed::build(rows.iter().map(|&(s, t, k)| (t, s, k)).collect())?;
        let forward = Keyed::build(rows)?;
        let index = Self {
            refsets,
            forward,
            backward,
        };
        index.validate(store.ids.len())?;
        Ok((index, skipped))
    }

    pub fn rows(&self) -> usize {
        self.forward.others.len()
    }

    pub fn refsets(&self) -> &[u64] {
        &self.refsets
    }

    fn collect(&self, (others, kinds): (&[u32], &[u8])) -> Vec<Association> {
        others
            .iter()
            .zip(kinds)
            .map(|(&concept, &kind)| Association {
                concept,
                refset: self.refsets[kind as usize],
            })
            .collect()
    }

    /// What `concept` was retired in favour of, by association.
    pub fn successors(&self, concept: u32) -> Vec<Association> {
        self.collect(self.forward.get(concept))
    }

    /// The retired concepts that point at `concept`, by association.
    pub fn predecessors(&self, concept: u32) -> Vec<Association> {
        self.collect(self.backward.get(concept))
    }

    /// Calls `visit` with each of `concepts` that has rows, and those rows.
    ///
    /// `concepts` is sorted, as are the keys, so a large input is merged with
    /// the key list in one pass while a small one binary-searches it. Either
    /// way the cost follows whichever of the two lists is shorter.
    pub(crate) fn walk(
        &self,
        backward: bool,
        concepts: &[u32],
        mut visit: impl FnMut(u32, &[u32], &[u8]),
    ) {
        let keyed = if backward {
            &self.backward
        } else {
            &self.forward
        };
        let rows = |i: usize| {
            let range = keyed.offsets[i] as usize..keyed.offsets[i + 1] as usize;
            (&keyed.others[range.clone()], &keyed.kinds[range])
        };
        if concepts.len().saturating_mul(16) < keyed.keys.len() {
            for &concept in concepts {
                if let Ok(i) = keyed.keys.binary_search(&concept) {
                    let (others, kinds) = rows(i);
                    visit(concept, others, kinds);
                }
            }
        } else {
            let (mut a, mut b) = (0, 0);
            while a < concepts.len() && b < keyed.keys.len() {
                match concepts[a].cmp(&keyed.keys[b]) {
                    std::cmp::Ordering::Less => a += 1,
                    std::cmp::Ordering::Greater => b += 1,
                    std::cmp::Ordering::Equal => {
                        let (others, kinds) = rows(b);
                        visit(concepts[a], others, kinds);
                        a += 1;
                        b += 1;
                    }
                }
            }
        }
    }

    pub(crate) fn kind_of(&self, refset: u64) -> Option<u8> {
        self.refsets
            .iter()
            .position(|&r| r == refset)
            .map(|i| i as u8)
    }

    fn validate(&self, concepts: usize) -> Result<()> {
        self.forward.validate(concepts, self.refsets.len())?;
        self.backward.validate(concepts, self.refsets.len())?;
        ensure!(
            self.forward.others.len() == self.backward.others.len(),
            "History directions disagree on the number of rows"
        );
        Ok(())
    }

    pub fn write(&self, path: &Path, skipped: usize) -> Result<HistoryManifest> {
        let mut out = BufWriter::new(File::create_new(path)?);
        out.write_all(MAGIC)?;
        put_u64(&mut out, self.refsets.len() as u64)?;
        for &refset in &self.refsets {
            put_u64(&mut out, refset)?;
        }
        self.forward.write(&mut out)?;
        self.backward.write(&mut out)?;
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(HistoryManifest {
            bytes: path.metadata()?.len(),
            sha256: sha256(path)?,
            rows: self.rows(),
            refsets: self.refsets.clone(),
            skipped,
        })
    }

    pub(super) fn open(
        section: &Section,
        manifest: &HistoryManifest,
        concepts: usize,
    ) -> Result<Self> {
        let mut input = Input::open(section, MAGIC)?;
        let count = input.count(8)?;
        let mut refsets = Vec::with_capacity(count);
        for _ in 0..count {
            refsets.push(input.u64()?);
        }
        let forward = Keyed::read(&mut input)?;
        let backward = Keyed::read(&mut input)?;
        ensure!(input.remaining == 0, "Trailing history bytes");
        let index = Self {
            refsets,
            forward,
            backward,
        };
        index.validate(concepts)?;
        ensure!(
            index.rows() == manifest.rows && index.refsets == manifest.refsets,
            "History index differs from manifest"
        );
        Ok(index)
    }
}

/// Builds the history index for a staged store and records it in the manifest.
///
/// Import, supplements and `build-history` all end here, so every path that
/// produces a store produces the same index from the same rows.
pub fn add_history(directory: &Path) -> Result<Option<HistoryManifest>> {
    let store = NumericStore::open(directory)?;
    if !store.member_tables.is_available() {
        return Ok(None);
    }
    let (index, skipped) = HistoryIndex::build(&store)?;
    drop(store);
    let path = directory.join("history.bin");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let written = index.write(&path, skipped)?;
    let mut manifest = Manifest::read(directory)?;
    manifest.history = Some(written.clone());
    let file = directory.join("manifest.json");
    let mut out = BufWriter::new(File::create(&file)?);
    serde_json::to_writer_pretty(&mut out, &manifest)?;
    out.flush()?;
    out.get_ref().sync_all()?;
    Ok(Some(written))
}

/// Opens the section on first use, like descriptions and the word index.
#[derive(Debug, Default)]
pub struct HistoryStore {
    source: Option<(Section, HistoryManifest, usize)>,
    loaded: OnceLock<std::result::Result<HistoryIndex, String>>,
}

impl HistoryStore {
    pub(super) fn lazy(
        source: &IndexSource,
        metadata: HistoryManifest,
        concepts: usize,
    ) -> Result<Self> {
        Ok(Self {
            source: Some((source.section("history.bin")?, metadata, concepts)),
            loaded: OnceLock::new(),
        })
    }
    /// For tests and for callers that built an index in memory.
    pub fn loaded(index: HistoryIndex) -> Self {
        Self {
            source: None,
            loaded: OnceLock::from(Ok(index)),
        }
    }
    pub fn get(&self) -> Result<Option<&HistoryIndex>> {
        if self.source.is_none() && self.loaded.get().is_none() {
            return Ok(None);
        }
        match self.loaded.get_or_init(|| {
            let (section, manifest, concepts) = self.source.as_ref().unwrap();
            HistoryIndex::open(section, manifest, *concepts).map_err(|e| e.to_string())
        }) {
            Ok(index) => Ok(Some(index)),
            Err(message) => bail!("History index: {message}"),
        }
    }
}
