//! Versioned container with independently bounded, checksum-verified sections.
use super::*;
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

const MAGIC: &[u8; 8] = b"SNECL002";
const HEADER: u64 = 56;
const PAGE: u64 = 4096;
const MAX_TABLE: u64 = 4 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    manifest: Manifest,
    sections: Vec<Entry>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    offset: u64,
    length: u64,
    codec: u8,
}

#[derive(Clone, Debug)]
pub(crate) struct Section {
    path: PathBuf,
    offset: u64,
    pub(super) length: u64,
    encoded_length: u64,
    codec: u8,
    file_length: u64,
    sha256: String,
}
impl Section {
    pub(super) fn reader(&self) -> Result<SectionReader> {
        let mut file = File::open(&self.path)?;
        ensure!(
            file.metadata()?.len() == self.file_length,
            "Store file size mismatch"
        );
        file.seek(SeekFrom::Start(self.offset))?;
        let blocks = match self.codec {
            0 => None,
            1 => Some(super::blocks::Blocks::open(
                &mut file,
                self.offset,
                self.encoded_length,
                self.length,
            )?),
            _ => bail!("Unsupported section codec"),
        };
        Ok(SectionReader {
            file,
            blocks,
            start: self.offset,
            length: self.length,
            position: 0,
        })
    }
    pub(super) fn verify(&self) -> Result<()> {
        let mut reader = self.reader()?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 1024 * 1024];
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        ensure!(reader.position == self.length, "Truncated store section");
        ensure!(
            format!("{:x}", hasher.finalize()) == self.sha256,
            "Store checksum mismatch"
        );
        Ok(())
    }
    #[cfg(feature = "import")]
    pub(crate) fn copy_to(&self, destination: &Path) -> Result<()> {
        let mut output = File::create_new(destination)?;
        ensure!(
            io::copy(&mut self.reader()?, &mut output)? == self.length,
            "Truncated store section"
        );
        output.sync_all()?;
        ensure!(
            sha256(destination)? == self.sha256,
            "Copied section checksum mismatch"
        );
        Ok(())
    }
}

/// Owns its file handle. Concurrent readers never share a seek cursor.
#[derive(Debug)]
pub(super) struct SectionReader {
    file: File,
    blocks: Option<super::blocks::Blocks>,
    start: u64,
    length: u64,
    position: u64,
}
impl Read for SectionReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = bytes
            .len()
            .min((self.length - self.position).min(usize::MAX as u64) as usize);
        let read = if let Some(blocks) = &mut self.blocks {
            blocks.read(&mut self.file, self.position, &mut bytes[..count])?
        } else {
            self.file.read(&mut bytes[..count])?
        };
        self.position += read as u64;
        Ok(read)
    }
}
impl Seek for SectionReader {
    fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
        let position = match target {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::End(n) => self.length as i128 + n as i128,
            SeekFrom::Current(n) => self.position as i128 + n as i128,
        };
        if !(0..=self.length as i128).contains(&position) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Seek outside store section",
            ));
        }
        if self.blocks.is_none() {
            self.file
                .seek(SeekFrom::Start(self.start + position as u64))?;
        }
        self.position = position as u64;
        Ok(self.position)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct IndexSource {
    sections: Arc<BTreeMap<String, Section>>,
}
impl IndexSource {
    pub(crate) fn open(path: &Path) -> Result<(Manifest, Self)> {
        if path.is_dir() {
            let file = File::open(path.join("manifest.json"))?;
            ensure!(
                file.metadata()?.len() < 1024 * 1024,
                "Manifest is too large"
            );
            let manifest: Manifest = serde_json::from_reader(BufReader::new(file))?;
            let sections = specs(&manifest)?
                .into_iter()
                .map(|(name, length, hash)| {
                    let section = Section {
                        path: path.join(&name),
                        offset: 0,
                        length,
                        encoded_length: length,
                        codec: 0,
                        file_length: length,
                        sha256: hash,
                    };
                    (name, section)
                })
                .collect();
            return Ok((
                manifest,
                Self {
                    sections: Arc::new(sections),
                },
            ));
        }
        let mut input = File::open(path)?;
        let file_length = input.metadata()?.len();
        let mut header = [0; HEADER as usize];
        input.read_exact(&mut header)?;
        ensure!(&header[..8] == MAGIC, "Unsupported container header");
        let declared = u64::from_le_bytes(header[8..16].try_into()?);
        let table_length = u64::from_le_bytes(header[16..24].try_into()?);
        ensure!(declared == file_length, "Container file size mismatch");
        ensure!(
            table_length > 0
                && table_length <= MAX_TABLE
                && table_length <= file_length.saturating_sub(HEADER),
            "Invalid container table length"
        );
        let mut bytes = vec![0; table_length as usize];
        input.read_exact(&mut bytes)?;
        ensure!(
            Sha256::digest(&bytes)[..] == header[24..56],
            "Container table checksum mismatch"
        );
        let table: Table = serde_json::from_slice(&bytes)?;
        let expected = specs(&table.manifest)?;
        ensure!(
            expected.len() == table.sections.len(),
            "Container sections differ from manifest"
        );
        let mut sections = BTreeMap::new();
        let mut next = align(HEADER + table_length)?;
        for (entry, (name, length, hash)) in table.sections.into_iter().zip(expected) {
            ensure!(
                entry.name == name
                    && entry.offset == next
                    && entry.length > 0
                    && (entry.codec == 0 && entry.length == length || entry.codec == 1),
                "Invalid container section name, length or offset"
            );
            let end = entry
                .offset
                .checked_add(entry.length)
                .context("Section offset overflow")?;
            ensure!(end <= file_length, "Truncated container section");
            next = align(end)?;
            ensure!(
                sections
                    .insert(
                        name,
                        Section {
                            path: path.into(),
                            offset: entry.offset,
                            length,
                            encoded_length: entry.length,
                            codec: entry.codec,
                            file_length,
                            sha256: hash
                        }
                    )
                    .is_none(),
                "Duplicate container section"
            );
        }
        ensure!(
            next == file_length,
            "Trailing container bytes or missing padding"
        );
        Ok((
            table.manifest,
            Self {
                sections: Arc::new(sections),
            },
        ))
    }
    pub(crate) fn section(&self, name: &str) -> Result<Section> {
        self.sections
            .get(name)
            .cloned()
            .with_context(|| format!("Required index section is absent: {name}"))
    }
}

fn specs(manifest: &Manifest) -> Result<Vec<(String, u64, String)>> {
    ensure!(manifest.format == FORMAT, "Unsupported store format");
    let mut specs = vec![(
        "core.bin".into(),
        manifest.core_bytes,
        manifest.core_sha256.clone(),
    )];
    if let Some(m) = &manifest.membership {
        specs.push(("membership.bin".into(), m.bytes, m.sha256.clone()));
    }
    if let Some(tables) = &manifest.member_tables {
        let mut tables: Vec<_> = tables.iter().collect();
        tables.sort_unstable_by_key(|m| m.refset);
        ensure!(
            tables.windows(2).all(|w| w[0].refset < w[1].refset),
            "Duplicate member manifest"
        );
        for m in tables {
            specs.push((
                format!("members/{}.bin", m.refset),
                m.bytes,
                m.sha256.clone(),
            ));
        }
    }
    if let Some(m) = &manifest.identifiers {
        specs.push(("identifiers.json".into(), m.bytes, m.sha256.clone()));
    }
    if let Some(m) = &manifest.descriptions {
        specs.push(("descriptions.bin".into(), m.bytes, m.sha256.clone()));
    }
    if manifest.display_bytes > 0 {
        specs.push((
            "display.bin".into(),
            manifest.display_bytes,
            manifest.display_sha256.clone(),
        ));
    }
    for (_, length, hash) in &specs {
        ensure!(
            *length > 0 && *length <= 2 * 1024 * 1024 * 1024,
            "Unsupported section size"
        );
        ensure!(
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "Invalid section SHA-256"
        );
    }
    Ok(specs)
}
fn align(value: u64) -> Result<u64> {
    Ok(value
        .checked_add(PAGE - 1)
        .context("Container size overflow")?
        / PAGE
        * PAGE)
}

/// Packs verified component bytes into a new file. Existing destinations are never replaced.
pub fn pack(source: &Path, destination: &Path) -> Result<()> {
    pack_with_options(source, destination, PackOptions::default())
}

#[derive(Clone, Copy, Debug)]
pub struct PackOptions {
    pub compress: bool,
    pub block_bytes: u32,
}
impl Default for PackOptions {
    fn default() -> Self {
        Self {
            compress: true,
            block_bytes: 65536,
        }
    }
}

pub fn pack_with_options(source: &Path, destination: &Path, options: PackOptions) -> Result<()> {
    ensure!(!destination.exists(), "Destination already exists");
    ensure!(
        super::blocks::valid_size(options.block_bytes),
        "Invalid compression block size"
    );
    let (manifest, source) = IndexSource::open(source)?;
    let entries = specs(&manifest)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let spool_path = destination.with_extension(format!("spool-{}-{stamp}", std::process::id()));
    let mut spool = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&spool_path)?;
    let spool_guard = Pending(spool_path);
    let mut encoded = Vec::new();
    let mut offsets = Vec::new();
    for (name, length, _) in entries {
        let section = source.section(&name)?;
        section.verify()?;
        offsets.push(spool.stream_position()?);
        let encoded_length = if options.compress {
            super::blocks::encode(
                &mut section.reader()?,
                &mut spool,
                length,
                options.block_bytes,
            )?
        } else {
            let copied = io::copy(&mut section.reader()?, &mut spool)?;
            ensure!(copied == length, "Truncated source section");
            copied
        };
        encoded.push(Entry {
            name,
            offset: 0,
            length: encoded_length,
            codec: u8::from(options.compress),
        });
    }
    let mut table = Table {
        manifest,
        sections: encoded,
    };
    let mut start = PAGE;
    let (bytes, length) = loop {
        let mut next = start;
        for entry in &mut table.sections {
            entry.offset = next;
            next = align(
                next.checked_add(entry.length)
                    .context("Container size overflow")?,
            )?;
        }
        let bytes = serde_json::to_vec(&table)?;
        ensure!(
            bytes.len() as u64 <= MAX_TABLE,
            "Container table is too large"
        );
        let required = align(HEADER + bytes.len() as u64)?;
        if required == start {
            break (bytes, next);
        }
        start = required;
    };
    let staging = destination.with_extension(format!("partial-{}-{stamp}", std::process::id()));
    let mut output = File::create_new(&staging)?;
    let pending = Pending(staging);
    output.write_all(MAGIC)?;
    output.write_all(&length.to_le_bytes())?;
    output.write_all(&(bytes.len() as u64).to_le_bytes())?;
    output.write_all(&Sha256::digest(&bytes))?;
    output.write_all(&bytes)?;
    for (entry, offset) in table.sections.iter().zip(offsets) {
        spool.seek(SeekFrom::Start(offset))?;
        output.seek(SeekFrom::Start(entry.offset))?;
        ensure!(
            io::copy(&mut (&mut spool).take(entry.length), &mut output)? == entry.length,
            "Truncated source section"
        );
    }
    output.set_len(length)?;
    output.sync_all()?;
    // Also checks copied bytes, including source changes during the copy.
    let (_, written) = IndexSource::open(&pending.0)?;
    for section in written.sections.values() {
        section.verify()?;
    }
    drop(output);
    drop(spool);
    drop(spool_guard);
    // A hard link publishes atomically and fails if the destination appeared meanwhile.
    std::fs::hard_link(&pending.0, destination)?;
    Ok(())
}

struct Pending(PathBuf);
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[derive(Debug, Serialize)]
pub struct Verification {
    pub sections: usize,
    pub component_bytes: u64,
    pub concepts: usize,
    pub descriptions: usize,
    pub member_rows: usize,
    pub identifiers: usize,
}

/// Exhaustively verifies checksums and structure, loading each cold section separately.
pub fn verify(path: &Path) -> Result<Verification> {
    let (manifest, source) = IndexSource::open(path)?;
    let store = NumericStore::open(path)?;
    // Opening only checks bounds, so verification does the semantic pass.
    store.validate()?;
    let mut result = Verification {
        sections: source.sections.len(),
        component_bytes: source.sections.values().map(|s| s.length).sum(),
        concepts: store.ids.len(),
        descriptions: 0,
        member_rows: 0,
        identifiers: 0,
    };
    if let Some(meta) = &manifest.descriptions {
        result.descriptions =
            DescriptionIndex::open(&source.section("descriptions.bin")?, meta, store.ids.len())?
                .len();
    }
    if let Some(tables) = &manifest.member_tables {
        for meta in tables {
            result.member_rows += MemberTable::open(&source, meta)?.len();
        }
    }
    if let Some(meta) = &manifest.identifiers {
        result.identifiers = IdentifierIndex::open(&source.section("identifiers.json")?, meta)?
            .rows
            .len();
    }
    if manifest.display_bytes > 0 {
        DisplayStore::open(path)?.verify_text()?;
    }
    Ok(result)
}
