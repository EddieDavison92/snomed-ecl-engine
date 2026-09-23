//! One display label per concept, read by ordinal on demand.
//!
//! Each label is its own zstd frame, compressed against a dictionary trained on
//! a sample of the labels. Labels are too short to compress alone, and a block
//! shared by many labels costs a whole block decode to read one; a dictionary
//! gives most of the saving of block compression at the cost of one small frame.
//! The uncompressed lengths are kept in memory, so search can rank candidates by
//! label length without reading them.
use super::container::{PositionalReader, Section, SectionReader};
use super::{validate_offsets, IndexSource, Input};
use anyhow::{ensure, Context, Result};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

const MAGIC_V1: &[u8; 8] = b"SNDSP001";
const MAGIC: &[u8; 8] = b"SNDSP002";

/// Largest dictionary trained, in bytes. Beyond about 100 KiB the UK labels
/// shrink by under a megabyte more.
#[cfg(feature = "import")]
const DICTIONARY_BYTES: usize = 112 * 1024;
/// Label bytes sampled for training; zstd advises about 100 times the dictionary.
#[cfg(feature = "import")]
const SAMPLE_BYTES: usize = 100 * DICTIONARY_BYTES;
/// Too few labels to train on, as in test fixtures: frames then use no dictionary.
#[cfg(feature = "import")]
const MINIMUM_SAMPLES: usize = 1000;
#[cfg(feature = "import")]
const LEVEL: i32 = 19;

pub struct DisplayStore {
    /// Reads labels without a lock when the section is uncompressed.
    positional: Option<PositionalReader>,
    /// Otherwise shared under a lock, decoding a block per read.
    input: Mutex<BufReader<SectionReader>>,
    /// Where each label's stored bytes start, relative to `start`.
    offsets: Vec<u32>,
    /// Present for compressed labels; a legacy file stores them as plain text.
    frames: Option<Frames>,
    start: u64,
}

struct Frames {
    lengths: Vec<u16>,
    dictionary: Vec<u8>,
    /// Idle decoders, one per concurrent reader at most. Loading the
    /// dictionary costs more than decoding a label, so decoders are reused.
    decoders: Mutex<Vec<zstd::bulk::Decompressor<'static>>>,
}

impl Frames {
    fn decode(&self, frame: &[u8], length: usize) -> Result<Vec<u8>> {
        let idle = self
            .decoders
            .lock()
            .map_err(|_| anyhow::anyhow!("Display decoders poisoned"))?
            .pop();
        let mut decoder = match idle {
            Some(decoder) => decoder,
            None => zstd::bulk::Decompressor::with_dictionary(&self.dictionary)?,
        };
        let mut bytes = Vec::with_capacity(length);
        decoder
            .decompress_to_buffer(frame, &mut bytes)
            .context("Invalid display label")?;
        ensure!(bytes.len() == length, "Display label length mismatch");
        if let Ok(mut idle) = self.decoders.lock() {
            idle.push(decoder);
        }
        Ok(bytes)
    }
}

impl DisplayStore {
    /// Decodes every label, checking each is valid UTF-8.
    pub(super) fn verify_text(&mut self) -> Result<()> {
        self.all_labels().map(drop)
    }

    fn all_labels(&mut self) -> Result<Vec<Option<String>>> {
        let input = self.input.get_mut().expect("display reader poisoned");
        input.seek(SeekFrom::Start(self.start))?;
        let mut text = Vec::new();
        input.read_to_end(&mut text)?;
        (0..self.offsets.len() - 1)
            .map(|i| {
                let stored = &text[self.offsets[i] as usize..self.offsets[i + 1] as usize];
                self.label(i, stored)
            })
            .collect()
    }

    fn label(&self, i: usize, stored: &[u8]) -> Result<Option<String>> {
        if stored.is_empty() {
            return Ok(None);
        }
        let bytes = match &self.frames {
            Some(frames) => frames.decode(stored, frames.lengths[i] as usize)?,
            None => stored.to_vec(),
        };
        Ok(Some(
            String::from_utf8(bytes).context("Invalid display UTF-8")?,
        ))
    }

    #[cfg(feature = "import")]
    pub(crate) fn into_labels(mut self) -> Result<Vec<Option<String>>> {
        self.all_labels()
    }

    #[cfg(feature = "import")]
    pub fn write(path: &Path, labels: &[Option<String>]) -> Result<()> {
        use super::{put_u32s, put_u64};
        use std::io::{BufWriter, Write};
        let present: Vec<&[u8]> = labels.iter().flatten().map(|s| s.as_bytes()).collect();
        let dictionary = if present.len() < MINIMUM_SAMPLES {
            Vec::new()
        } else {
            // Every nth label, so the sample spans the hierarchy without randomness.
            let total: usize = present.iter().map(|l| l.len()).sum();
            let step = total.div_ceil(SAMPLE_BYTES).max(1);
            let mut sample = Vec::new();
            let mut sizes = Vec::new();
            for label in present.iter().step_by(step) {
                sample.extend_from_slice(label);
                sizes.push(label.len());
            }
            zstd::dict::from_continuous(&sample, &sizes, DICTIONARY_BYTES)
                .context("Could not train the display dictionary")?
        };
        let mut compressor = zstd::bulk::Compressor::with_dictionary(LEVEL, &dictionary)?;
        compressor.include_checksum(false)?;
        compressor.include_contentsize(false)?;
        compressor.include_dictid(false)?;

        let mut offsets = Vec::with_capacity(labels.len() + 1);
        let mut lengths = Vec::with_capacity(labels.len());
        let mut frames = Vec::new();
        offsets.push(0u32);
        for label in labels {
            let bytes = label.as_deref().unwrap_or_default().as_bytes();
            lengths.push(u16::try_from(bytes.len()).context("Display label too long")?);
            if !bytes.is_empty() {
                frames.extend_from_slice(&compressor.compress(bytes)?);
            }
            offsets
                .push(u32::try_from(frames.len()).context("Display section exceeds u32 capacity")?);
        }
        let mut out = BufWriter::new(std::fs::File::create_new(path)?);
        out.write_all(MAGIC)?;
        put_u32s(&mut out, &offsets)?;
        put_u64(&mut out, lengths.len() as u64)?;
        for length in &lengths {
            out.write_all(&length.to_le_bytes())?;
        }
        put_u64(&mut out, dictionary.len() as u64)?;
        out.write_all(&dictionary)?;
        out.write_all(&frames)?;
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(())
    }

    pub fn open(directory: &Path) -> Result<Self> {
        let (manifest, source) = IndexSource::open(directory)?;
        Self::from_section(&source.section("display.bin")?, manifest.concept_count)
    }

    fn from_section(section: &Section, concept_count: usize) -> Result<Self> {
        let (mut input, version) = Input::open_versions(section, &[MAGIC_V1, MAGIC])?;
        let offsets = input.u32s()?;
        let frames = if version == 0 {
            None
        } else {
            let lengths = input.u16s()?;
            ensure!(
                lengths.len() == concept_count
                    && lengths
                        .iter()
                        .zip(offsets.windows(2))
                        .all(|(&length, w)| (length == 0) == (w[0] == w[1])),
                "Invalid display lengths"
            );
            let dictionary = input.bytes()?;
            Some(Frames {
                lengths,
                dictionary,
                decoders: Mutex::new(Vec::new()),
            })
        };
        validate_offsets(&offsets, concept_count, input.remaining as usize)?;
        let start = input.reader.stream_position()?;
        Ok(Self {
            positional: section.positional()?,
            input: Mutex::new(input.reader),
            offsets,
            frames,
            start,
        })
    }

    /// Starts reading every label in the background, so a server's first
    /// searches do not wait on the disk for each one. Does nothing for a
    /// compressed section.
    pub fn prefetch(&self) -> Result<()> {
        match &self.positional {
            Some(reader) => reader.prefetch(),
            None => Ok(()),
        }
    }

    /// Byte length of a concept's label, without reading it.
    ///
    /// The lengths are already in memory, so ranking thousands of candidates
    /// by label length costs nothing. Only the survivors are then read.
    pub fn label_bytes(&self, ordinal: u32) -> Option<u32> {
        let i = ordinal as usize;
        match &self.frames {
            Some(frames) => frames.lengths.get(i).map(|&n| u32::from(n)),
            None => Some(self.offsets.get(i + 1)? - self.offsets.get(i)?),
        }
    }

    pub fn get(&self, ordinal: u32) -> Result<Option<String>> {
        let i = ordinal as usize;
        ensure!(i + 1 < self.offsets.len(), "Display ordinal out of range");
        let length = (self.offsets[i + 1] - self.offsets[i]) as usize;
        if length == 0 {
            return Ok(None);
        }
        let position = self.start + self.offsets[i] as u64;
        let mut stored = vec![0; length];
        if let Some(reader) = &self.positional {
            reader.read_exact_at(position, &mut stored)?;
        } else {
            let mut input = self
                .input
                .lock()
                .map_err(|_| anyhow::anyhow!("Display reader poisoned"))?;
            input.seek(SeekFrom::Start(position))?;
            input.read_exact(&mut stored)?;
        }
        self.label(i, &stored)
    }
}

#[cfg(all(test, feature = "import"))]
mod tests {
    use super::*;

    fn open(path: &Path) -> Result<DisplayStore> {
        let length = std::fs::metadata(path)?.len();
        DisplayStore::from_section(&Section::for_test(path, length, String::new()), 2001)
    }

    #[test]
    fn labels_round_trip_through_a_trained_dictionary() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("display.bin");
        // Enough labels to train on; one concept has none.
        let labels: Vec<Option<String>> = (0..2001)
            .map(|i| {
                (i != 7).then(|| format!("Synthetic finding {i} of left é structure (disorder)"))
            })
            .collect();
        DisplayStore::write(&path, &labels).unwrap();
        let store = open(&path).unwrap();
        assert!(!store.frames.as_ref().unwrap().dictionary.is_empty());
        for (i, label) in labels.iter().enumerate() {
            assert_eq!(&store.get(i as u32).unwrap(), label);
            assert_eq!(
                store.label_bytes(i as u32),
                Some(label.as_ref().map_or(0, |l| l.len() as u32))
            );
        }
        assert!(store.get(2001).is_err());
        assert_eq!(open(&path).unwrap().into_labels().unwrap(), labels);

        // A damaged frame fails rather than returning other text.
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 2;
        bytes[last] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();
        let damaged = open(&path).unwrap();
        assert!((0..2001).any(|i| damaged.get(i).is_err()));
    }
}
