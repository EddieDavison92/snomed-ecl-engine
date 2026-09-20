use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
mod blocks;
mod columns;
mod container;
mod descriptions;
mod identifiers;
mod members;
mod membership;
mod term_storage;
pub(crate) use container::IndexSource;
pub use container::{pack, pack_with_options, verify, PackOptions, Verification};
use container::{Section, SectionReader};
pub use descriptions::{Description, DescriptionIndex, DescriptionManifest, DescriptionStore};
pub use identifiers::{Identifier, IdentifierIndex, IdentifierManifest, IdentifierStore};
pub use members::{
    format_uuid, is_concept_id, parse_uuid, MemberColumn, MemberManifest, MemberStore, MemberTable,
    MemberValue, TextColumn,
};
pub use membership::{MembershipIndex, MembershipManifest};

pub const FORMAT: u32 = 1;
const CORE_MAGIC: &[u8; 8] = b"SNECL001";
const DISPLAY_MAGIC: &[u8; 8] = b"SNDSP001";

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub edition: String,
    pub archive_sha256: String,
    pub concept_count: usize,
    pub active_concept_count: usize,
    pub hierarchy_edges: usize,
    pub attributes: usize,
    pub concrete_attributes: usize,
    pub concrete_values: usize,
    pub core_bytes: u64,
    pub core_sha256: String,
    pub display_bytes: u64,
    pub display_sha256: String,
    pub display_refsets: Vec<u64>,
    pub displays_selected: usize,
    pub module_dependencies: serde_json::Value,
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membership: Option<MembershipManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptions: Option<DescriptionManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_tables: Option<Vec<MemberManifest>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifiers: Option<IdentifierManifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supplements: Vec<RefsetSupplement>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefsetSupplement {
    pub archive_sha256: String,
    pub release_date: u32,
    pub base_core_sha256: String,
    pub refset_ids: Vec<String>,
    pub added_concepts: usize,
    pub module_dependencies: serde_json::Value,
    #[serde(default)]
    pub exact_module_versions_verified: bool,
}

impl Manifest {
    pub fn read(directory: &Path) -> Result<Self> {
        IndexSource::open(directory).map(|(manifest, _)| manifest)
    }
}

#[derive(Debug, Default)]
pub struct Adjacency {
    pub offsets: Vec<u32>,
    pub values: Vec<u32>,
}

impl Adjacency {
    pub fn build(count: usize, mut pairs: Vec<(u32, u32)>) -> Result<Self> {
        pairs.sort_unstable();
        pairs.dedup();
        let mut result = Self {
            offsets: vec![0; count + 1],
            values: Vec::with_capacity(pairs.len()),
        };
        for (source, target) in pairs {
            ensure!(
                (source as usize) < count && (target as usize) < count,
                "Invalid graph endpoint"
            );
            result.offsets[source as usize + 1] += 1;
            result.values.push(target);
        }
        prefix_sum(&mut result.offsets)?;
        Ok(result)
    }

    pub fn get(&self, ordinal: u32) -> &[u32] {
        &self.values
            [self.offsets[ordinal as usize] as usize..self.offsets[ordinal as usize + 1] as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Attribute {
    pub group: u32,
    pub kind: u32,
    pub value: u32,
}

#[derive(Debug, Default)]
pub struct Attributes {
    pub offsets: Vec<u32>,
    pub rows: Vec<Attribute>,
}

impl Attributes {
    pub fn build(count: usize, mut rows: Vec<(u32, Attribute)>) -> Result<Self> {
        rows.sort_unstable();
        let mut result = Self {
            offsets: vec![0; count + 1],
            rows: Vec::with_capacity(rows.len()),
        };
        for (source, attribute) in rows {
            ensure!((source as usize) < count, "Invalid attribute source");
            result.offsets[source as usize + 1] += 1;
            result.rows.push(attribute);
        }
        prefix_sum(&mut result.offsets)?;
        Ok(result)
    }

    pub fn get(&self, ordinal: u32) -> &[Attribute] {
        &self.rows
            [self.offsets[ordinal as usize] as usize..self.offsets[ordinal as usize + 1] as usize]
    }
}

fn prefix_sum(offsets: &mut [u32]) -> Result<()> {
    for i in 1..offsets.len() {
        offsets[i] = offsets[i]
            .checked_add(offsets[i - 1])
            .context("Index exceeds u32 capacity")?;
    }
    Ok(())
}

/// Concrete values retain their RF2 spelling, including decimal precision.
/// They are semantic values in the core, not display labels.
#[derive(Debug, PartialEq, Eq)]
pub enum ConcreteValue {
    Number(String),
    Text(String),
    Boolean(bool),
}

impl ConcreteValue {
    pub fn parse(wire: &str) -> Result<Self> {
        if let Some(number) = wire.strip_prefix('#') {
            let number = number
                .strip_prefix('-')
                .or_else(|| number.strip_prefix('+'))
                .unwrap_or(number);
            let parts: Vec<_> = number.split('.').collect();
            ensure!(
                parts.len() <= 2
                    && parts
                        .iter()
                        .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit())),
                "Invalid concrete decimal"
            );
            Ok(Self::Number(wire.to_owned()))
        } else if wire.len() >= 2 && wire.starts_with('"') && wire.ends_with('"') {
            Ok(Self::Text(wire.to_owned()))
        } else if wire == "true" || wire == "false" {
            Ok(Self::Boolean(wire == "true"))
        } else {
            bail!("Unsupported concrete value encoding")
        }
    }

    fn wire(&self) -> &str {
        match self {
            Self::Number(s) | Self::Text(s) => s,
            Self::Boolean(true) => "true",
            Self::Boolean(false) => "false",
        }
    }
}

#[derive(Debug, Default)]
pub struct NumericStore {
    pub ids: Vec<u64>,
    pub modules: Vec<u32>,
    pub effective_times: Vec<u32>,
    /// Bit 0: active. Bit 1: fully defined.
    pub flags: Vec<u8>,
    pub parents: Adjacency,
    pub children: Adjacency,
    pub attributes: Attributes,
    pub concrete: Attributes,
    pub concrete_values: Vec<ConcreteValue>,
    /// Active refset member rows referencing concepts. None means the index was not built.
    pub membership: Option<MembershipIndex>,
    pub descriptions: DescriptionStore,
    pub member_tables: MemberStore,
    pub identifiers: IdentifierStore,
    pub config: crate::config::QueryConfig,
}

impl NumericStore {
    pub fn ordinal(&self, sctid: u64) -> Option<u32> {
        self.ids.binary_search(&sctid).ok().map(|i| i as u32)
    }
    pub fn is_active(&self, ordinal: u32) -> bool {
        self.flags[ordinal as usize] & 1 != 0
    }

    /// Returns numeric-sorted SCTIDs through active edges, including an inactive self if requested.
    pub fn hierarchy(
        &self,
        sctid: u64,
        ancestors: bool,
        direct: bool,
        include_self: bool,
    ) -> Vec<u64> {
        let Some(start) = self.ordinal(sctid) else {
            return Vec::new();
        };
        let edges = if ancestors {
            &self.parents
        } else {
            &self.children
        };
        let mut visited = vec![false; self.ids.len()];
        let mut stack = vec![start];
        visited[start as usize] = true;
        let mut matches = Vec::new();
        if include_self {
            matches.push(sctid);
        }
        while let Some(source) = stack.pop() {
            for &target in edges.get(source) {
                if !visited[target as usize] && self.is_active(target) {
                    visited[target as usize] = true;
                    matches.push(self.ids[target as usize]);
                    if !direct {
                        stack.push(target);
                    }
                }
            }
        }
        matches.sort_unstable();
        matches
    }

    /// Checks that every stored index stays inside its own arrays.
    ///
    /// Opening a store runs only these. They are what later evaluation relies
    /// on to index safely; the semantic invariants in `validate` were proved
    /// when the index was written, and the section checksum already shows the
    /// bytes are the same ones.
    pub fn validate_bounds(&self) -> Result<()> {
        let n = self.ids.len();
        if let Some(membership) = &self.membership {
            membership.validate(n)?;
        }
        ensure!(n > 0 && n < u32::MAX as usize, "Invalid concept count");
        ensure!(
            self.modules.len() == n && self.effective_times.len() == n && self.flags.len() == n,
            "Concept section length mismatch"
        );
        ensure!(
            self.modules.iter().all(|&i| (i as usize) < n),
            "Missing module concept"
        );
        ensure!(self.flags.iter().all(|&f| f <= 3), "Invalid concept flags");
        for graph in [&self.parents, &self.children] {
            validate_offsets(&graph.offsets, n, graph.values.len())?;
            ensure!(
                graph.values.iter().all(|&i| (i as usize) < n),
                "Invalid hierarchy endpoint"
            );
        }
        ensure!(
            self.parents.values.len() == self.children.values.len(),
            "Hierarchy directions disagree"
        );
        for (index, value_count) in [
            (&self.attributes, n),
            (&self.concrete, self.concrete_values.len()),
        ] {
            validate_offsets(&index.offsets, n, index.rows.len())?;
            ensure!(
                index
                    .rows
                    .iter()
                    .all(|r| (r.kind as usize) < n && (r.value as usize) < value_count),
                "Invalid attribute reference"
            );
        }
        Ok(())
    }

    /// Checks the bounds above and then every semantic invariant: ordering,
    /// that the two hierarchy directions agree, that the graph is acyclic, and
    /// that active rows reference active concepts.
    ///
    /// Import runs this before publishing an index and `verify` runs it on
    /// demand. Opening a store does not, because re-deriving these properties
    /// on every process start costs more than it protects: the checksum
    /// detects the corruption they would otherwise catch.
    pub fn validate(&self) -> Result<()> {
        self.validate_bounds()?;
        let n = self.ids.len();
        ensure!(
            self.ids.windows(2).all(|w| w[0] < w[1]),
            "Duplicate or unordered concept IDs"
        );
        for graph in [&self.parents, &self.children] {
            for i in 0..n {
                ensure!(
                    graph.get(i as u32).windows(2).all(|w| w[0] < w[1]),
                    "Duplicate or unordered hierarchy edge"
                );
            }
        }
        for i in 0..n {
            for &parent in self.parents.get(i as u32) {
                ensure!(
                    self.is_active(i as u32) && self.is_active(parent),
                    "Active hierarchy references inactive concept"
                );
                ensure!(
                    self.children.get(parent).binary_search(&(i as u32)).is_ok(),
                    "Hierarchy directions disagree"
                );
            }
        }
        let mut pending: Vec<_> = (0..n).map(|i| self.parents.get(i as u32).len()).collect();
        let mut ready: VecDeque<_> = (0..n).filter(|&i| pending[i] == 0).collect();
        let mut processed = 0;
        while let Some(parent) = ready.pop_front() {
            processed += 1;
            for &child in self.children.get(parent as u32) {
                pending[child as usize] = pending[child as usize]
                    .checked_sub(1)
                    .context("Invalid hierarchy")?;
                if pending[child as usize] == 0 {
                    ready.push_back(child as usize);
                }
            }
        }
        ensure!(processed == n, "Hierarchy contains a cycle");
        for index in [&self.attributes, &self.concrete] {
            for i in 0..n {
                ensure!(
                    index.get(i as u32).windows(2).all(|w| w[0] <= w[1]),
                    "Unordered attribute group"
                );
                ensure!(
                    index.get(i as u32).is_empty() || self.is_active(i as u32),
                    "Active attribute references inactive source"
                );
            }
        }
        Ok(())
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let mut out = BufWriter::new(File::create_new(path)?);
        out.write_all(CORE_MAGIC)?;
        put_u64(&mut out, self.ids.len() as u64)?;
        for &id in &self.ids {
            put_u64(&mut out, id)?;
        }
        put_u32s(&mut out, &self.modules)?;
        put_u32s(&mut out, &self.effective_times)?;
        put_u64(&mut out, self.flags.len() as u64)?;
        out.write_all(&self.flags)?;
        for graph in [&self.parents, &self.children] {
            put_u32s(&mut out, &graph.offsets)?;
            put_u32s(&mut out, &graph.values)?;
        }
        for index in [&self.attributes, &self.concrete] {
            put_u32s(&mut out, &index.offsets)?;
            put_u64(&mut out, index.rows.len() as u64)?;
            for row in &index.rows {
                for value in [row.group, row.kind, row.value] {
                    put_u32(&mut out, value)?;
                }
            }
        }
        put_u64(&mut out, self.concrete_values.len() as u64)?;
        for value in &self.concrete_values {
            put_u64(&mut out, value.wire().len() as u64)?;
            out.write_all(value.wire().as_bytes())?;
        }
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(())
    }

    pub fn open(directory: &Path) -> Result<Self> {
        let (manifest, source) = IndexSource::open(directory)?;
        let mut input = Input::open(&source.section("core.bin")?, CORE_MAGIC)?;
        let count = input.count(8)?;
        let ids = input.records(count, 8, |b| {
            u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        })?;
        let modules = input.u32s()?;
        let effective_times = input.u32s()?;
        let flags = input.bytes()?;
        let mut graph = || -> Result<Adjacency> {
            Ok(Adjacency {
                offsets: input.u32s()?,
                values: input.u32s()?,
            })
        };
        let parents = graph()?;
        let children = graph()?;
        let mut attributes = || -> Result<Attributes> {
            let offsets = input.u32s()?;
            let count = input.count(12)?;
            let rows = input.records(count, 12, |b| Attribute {
                group: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                kind: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                value: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
            })?;
            Ok(Attributes { offsets, rows })
        };
        let attributes_index = attributes()?;
        let concrete = attributes()?;
        let value_count = input.count(8)?;
        let mut concrete_values = Vec::with_capacity(value_count);
        for _ in 0..value_count {
            concrete_values.push(ConcreteValue::parse(&String::from_utf8(input.bytes()?)?)?);
        }
        ensure!(input.remaining == 0, "Trailing store bytes");
        let store = Self {
            ids,
            modules,
            effective_times,
            flags,
            parents,
            children,
            attributes: attributes_index,
            concrete,
            concrete_values,
            membership: manifest
                .membership
                .as_ref()
                .map(|metadata| MembershipIndex::open(&source, metadata, count))
                .transpose()?,
            descriptions: manifest
                .descriptions
                .map(|m| DescriptionStore::lazy(&source, m, count))
                .transpose()?
                .unwrap_or_default(),
            member_tables: manifest
                .member_tables
                .map(|m| MemberStore::lazy(&source, m))
                .transpose()?
                .unwrap_or_default(),
            identifiers: manifest
                .identifiers
                .map(|m| IdentifierStore::lazy(&source, m))
                .transpose()?
                .unwrap_or_default(),
            config: crate::config::QueryConfig::default(),
        };
        store.validate_bounds()?;
        ensure!(
            store.ids.len() == manifest.concept_count
                && store.flags.iter().filter(|&&f| f & 1 != 0).count()
                    == manifest.active_concept_count,
            "Manifest concept counts differ"
        );
        ensure!(
            store.parents.values.len() == manifest.hierarchy_edges
                && store.attributes.rows.len() == manifest.attributes
                && store.concrete.rows.len() == manifest.concrete_attributes
                && store.concrete_values.len() == manifest.concrete_values,
            "Manifest relationship counts differ"
        );
        Ok(store)
    }
}

fn validate_offsets(offsets: &[u32], count: usize, values: usize) -> Result<()> {
    ensure!(
        offsets.len() == count + 1
            && offsets.first() == Some(&0)
            && offsets.last().copied().map(|n| n as usize) == Some(values),
        "Invalid index offsets"
    );
    ensure!(
        offsets.windows(2).all(|w| w[0] <= w[1]),
        "Non-monotonic index offsets"
    );
    Ok(())
}

/// Opens only the display file and its manifest; text is fetched by ordinal on demand.
pub struct DisplayStore {
    input: BufReader<SectionReader>,
    offsets: Vec<u32>,
    start: u64,
}

impl DisplayStore {
    fn verify_text(&mut self) -> Result<()> {
        self.input.seek(SeekFrom::Start(self.start))?;
        let mut text = String::new();
        self.input.read_to_string(&mut text)?;
        ensure!(
            self.offsets
                .iter()
                .all(|&v| text.is_char_boundary(v as usize)),
            "Invalid display UTF-8 offset"
        );
        Ok(())
    }
    #[cfg(feature = "import")]
    pub(crate) fn into_labels(mut self) -> Result<Vec<Option<String>>> {
        self.input.seek(SeekFrom::Start(self.start))?;
        let mut labels = Vec::with_capacity(self.offsets.len() - 1);
        for offsets in self.offsets.windows(2) {
            let mut bytes = vec![0; (offsets[1] - offsets[0]) as usize];
            self.input.read_exact(&mut bytes)?;
            labels.push(if bytes.is_empty() {
                None
            } else {
                Some(String::from_utf8(bytes)?)
            });
        }
        Ok(labels)
    }

    pub fn write(path: &Path, labels: &[Option<String>]) -> Result<()> {
        let mut offsets = Vec::with_capacity(labels.len() + 1);
        offsets.push(0u32);
        for label in labels {
            let size = label.as_ref().map_or(0, |s| s.len());
            offsets.push(
                offsets
                    .last()
                    .unwrap()
                    .checked_add(u32::try_from(size)?)
                    .context("Display section exceeds u32 capacity")?,
            );
        }
        let mut out = BufWriter::new(File::create_new(path)?);
        out.write_all(DISPLAY_MAGIC)?;
        put_u32s(&mut out, &offsets)?;
        for label in labels.iter().flatten() {
            out.write_all(label.as_bytes())?;
        }
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(())
    }

    pub fn open(directory: &Path) -> Result<Self> {
        let (manifest, source) = IndexSource::open(directory)?;
        let mut input = Input::open(&source.section("display.bin")?, DISPLAY_MAGIC)?;
        let offsets = input.u32s()?;
        validate_offsets(&offsets, manifest.concept_count, input.remaining as usize)?;
        let start = input.reader.stream_position()?;
        Ok(Self {
            input: input.reader,
            offsets,
            start,
        })
    }

    pub fn get(&mut self, ordinal: u32) -> Result<Option<String>> {
        let i = ordinal as usize;
        ensure!(i + 1 < self.offsets.len(), "Display ordinal out of range");
        let length = (self.offsets[i + 1] - self.offsets[i]) as usize;
        if length == 0 {
            return Ok(None);
        }
        self.input
            .seek(SeekFrom::Start(self.start + self.offsets[i] as u64))?;
        let mut bytes = vec![0; length];
        self.input.read_exact(&mut bytes)?;
        Ok(Some(String::from_utf8(bytes)?))
    }
}

pub fn sha256(path: &Path) -> Result<String> {
    let mut file = BufReader::new(File::open(path)?);
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn put_u64(out: &mut impl Write, value: u64) -> Result<()> {
    out.write_all(&value.to_le_bytes())?;
    Ok(())
}
fn put_u32(out: &mut impl Write, value: u32) -> Result<()> {
    out.write_all(&value.to_le_bytes())?;
    Ok(())
}
fn put_u32s(out: &mut impl Write, values: &[u32]) -> Result<()> {
    put_u64(out, values.len() as u64)?;
    for &value in values {
        put_u32(out, value)?;
    }
    Ok(())
}

struct Input {
    reader: BufReader<SectionReader>,
    remaining: u64,
}
impl Input {
    /// Opens a section without checksumming it.
    ///
    /// Hashing the section here read every byte before a query could run, and
    /// the decode below then read them all again. `verify` hashes every section
    /// explicitly instead, so integrity is still checked, just not on the path
    /// that only wants to answer a question.
    fn open(section: &Section, magic: &[u8; 8]) -> Result<Self> {
        let size = section.length;
        ensure!(
            (8..=2 * 1024 * 1024 * 1024).contains(&size),
            "Unsupported store file size"
        );
        let mut result = Self {
            // Large sequential reads reduce host/filesystem round trips at startup.
            reader: BufReader::with_capacity(1024 * 1024, section.reader()?),
            remaining: size,
        };
        let mut actual = [0; 8];
        result.read(&mut actual)?;
        ensure!(&actual == magic, "Unsupported store header");
        Ok(result)
    }
    fn read(&mut self, bytes: &mut [u8]) -> Result<()> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len() as u64)
            .context("Truncated store section")?;
        self.reader.read_exact(bytes)?;
        Ok(())
    }
    fn u64(&mut self) -> Result<u64> {
        let mut b = [0; 8];
        self.read(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn u32(&mut self) -> Result<u32> {
        let mut b = [0; 4];
        self.read(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn count(&mut self, width: u64) -> Result<usize> {
        let n = self.u64()?;
        ensure!(
            n.checked_mul(width)
                .is_some_and(|bytes| bytes <= self.remaining),
            "Invalid store section length"
        );
        Ok(usize::try_from(n)?)
    }
    /// Reads `n` fixed-width records through one scratch buffer.
    ///
    /// Pulling each record out of the reader separately costs a `read_exact`
    /// call per value, and the core holds tens of millions of them. Filling a
    /// buffer and decoding from the slice keeps the reads large and the decode
    /// loop tight, without allocating a second copy of the whole column.
    fn records<T>(
        &mut self,
        n: usize,
        width: usize,
        decode: impl Fn(&[u8]) -> T,
    ) -> Result<Vec<T>> {
        const SCRATCH: usize = 64 * 1024;
        let mut out = Vec::with_capacity(n);
        let per_pass = (SCRATCH / width).max(1);
        let mut scratch = vec![0; per_pass * width];
        let mut left = n;
        while left > 0 {
            let take = left.min(per_pass);
            let filled = &mut scratch[..take * width];
            self.read(filled)?;
            out.extend(filled.chunks_exact(width).map(&decode));
            left -= take;
        }
        Ok(out)
    }
    fn u32s(&mut self) -> Result<Vec<u32>> {
        let n = self.count(4)?;
        self.records(n, 4, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn bytes(&mut self) -> Result<Vec<u8>> {
        let n = self.count(1)?;
        let mut result = vec![0; n];
        self.read(&mut result)?;
        Ok(result)
    }
}
