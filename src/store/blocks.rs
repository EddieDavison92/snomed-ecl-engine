//! Independently compressed blocks with bounded decode buffers and random access.
use super::*;
use std::io;

const MAGIC: &[u8; 8] = b"SNZST001";
const HEADER: u64 = 16;
const ENTRY: u64 = 36;
/// zstd level for packing. Decoding costs the same at any level; packing the
/// UK edition at 15 takes under a minute and gives within 2% of level 19's
/// size, which takes five. Levels below 15 leave about a sixth more bytes.
const LEVEL: i32 = 15;

#[derive(Debug)]
struct Block {
    offset: u64,
    length: usize,
    sha256: [u8; 32],
}
#[derive(Debug)]
pub(super) struct Blocks {
    entries: Vec<Block>,
    block_bytes: usize,
    decoded_length: u64,
    cached: Option<usize>,
    decoded: Vec<u8>,
    encoded: Vec<u8>,
}
pub(super) fn valid_size(size: u32) -> bool {
    (4096..=1024 * 1024).contains(&size) && size.is_power_of_two()
}

impl Blocks {
    pub(super) fn open(
        file: &mut File,
        start: u64,
        encoded_length: u64,
        decoded_length: u64,
    ) -> Result<Self> {
        ensure!(encoded_length >= HEADER, "Truncated block header");
        let mut header = [0; HEADER as usize];
        file.read_exact(&mut header)?;
        ensure!(&header[..8] == MAGIC, "Unsupported block encoding");
        let block_bytes = u32::from_le_bytes(header[8..12].try_into()?);
        let count = u32::from_le_bytes(header[12..16].try_into()?) as u64;
        ensure!(
            valid_size(block_bytes) && count == decoded_length.div_ceil(block_bytes as u64),
            "Invalid compressed block count or size"
        );
        let table_bytes = count.checked_mul(ENTRY).context("Block table overflow")?;
        ensure!(
            HEADER + table_bytes <= encoded_length,
            "Truncated block table"
        );
        let mut table = vec![0; table_bytes as usize];
        file.read_exact(&mut table)?;
        let mut next = HEADER + table_bytes;
        let mut entries = Vec::with_capacity(count as usize);
        for entry in table.chunks_exact(ENTRY as usize) {
            let length = u32::from_le_bytes(entry[..4].try_into()?) as usize;
            ensure!(
                length > 0 && length <= block_bytes as usize * 2,
                "Invalid compressed block length"
            );
            let offset = next;
            next = next
                .checked_add(length as u64)
                .context("Block offset overflow")?;
            ensure!(next <= encoded_length, "Truncated compressed block");
            entries.push(Block {
                offset: start + offset,
                length,
                sha256: entry[4..].try_into()?,
            });
        }
        ensure!(next == encoded_length, "Trailing compressed bytes");
        Ok(Self {
            entries,
            block_bytes: block_bytes as usize,
            decoded_length,
            cached: None,
            decoded: vec![0; block_bytes as usize],
            encoded: Vec::new(),
        })
    }
    pub(super) fn read(
        &mut self,
        file: &mut File,
        position: u64,
        output: &mut [u8],
    ) -> io::Result<usize> {
        if output.is_empty() || position == self.decoded_length {
            return Ok(0);
        }
        let index = (position / self.block_bytes as u64) as usize;
        let inside = (position % self.block_bytes as u64) as usize;
        if self.cached != Some(index) {
            self.cached = None;
            let entry = &self.entries[index];
            self.encoded.resize(entry.length, 0);
            file.seek(SeekFrom::Start(entry.offset))?;
            file.read_exact(&mut self.encoded)?;
            if Sha256::digest(&self.encoded)[..] != entry.sha256 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Compressed block checksum mismatch",
                ));
            }
            let decoded = zstd::bulk::decompress_to_buffer(&self.encoded, &mut self.decoded)?;
            let expected = (self.decoded_length - index as u64 * self.block_bytes as u64)
                .min(self.block_bytes as u64) as usize;
            if decoded != expected {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Compressed block decoded length mismatch",
                ));
            }
            self.cached = Some(index);
        }
        let count = output
            .len()
            .min(self.block_bytes - inside)
            .min((self.decoded_length - position) as usize);
        output[..count].copy_from_slice(&self.decoded[inside..inside + count]);
        Ok(count)
    }
}

pub(super) fn encode(
    input: &mut impl Read,
    output: &mut File,
    length: u64,
    block_bytes: u32,
) -> Result<u64> {
    ensure!(
        valid_size(block_bytes),
        "Block size must be a power of two from 4096 to 1048576"
    );
    let start = output.stream_position()?;
    let count = u32::try_from(length.div_ceil(block_bytes as u64))?;
    let mut table = Vec::with_capacity(count as usize * ENTRY as usize);
    output.seek(SeekFrom::Start(start + HEADER + count as u64 * ENTRY))?;
    // Blocks are independent, so a batch is compressed across all cores and
    // written in order; the output does not depend on the thread count.
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut remaining = length;
    while remaining > 0 {
        let mut batch = Vec::new();
        while batch.len() < threads * 16 && remaining > 0 {
            let size = remaining.min(block_bytes as u64) as usize;
            let mut block = vec![0; size];
            input.read_exact(&mut block)?;
            batch.push(block);
            remaining -= size as u64;
        }
        for encoded in compress_all(&batch, threads)? {
            ensure!(
                encoded.len() <= block_bytes as usize * 2,
                "Compressed block is too large"
            );
            table.extend((encoded.len() as u32).to_le_bytes());
            table.extend(Sha256::digest(&encoded));
            output.write_all(&encoded)?;
        }
    }
    let end = output.stream_position()?;
    output.seek(SeekFrom::Start(start))?;
    output.write_all(MAGIC)?;
    output.write_all(&block_bytes.to_le_bytes())?;
    output.write_all(&count.to_le_bytes())?;
    output.write_all(&table)?;
    output.seek(SeekFrom::Start(end))?;
    Ok(end - start)
}

fn compress_all(blocks: &[Vec<u8>], threads: usize) -> Result<Vec<Vec<u8>>> {
    let per_thread = blocks.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let workers: Vec<_> = blocks
            .chunks(per_thread)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|block| zstd::bulk::compress(block, LEVEL))
                        .collect::<io::Result<Vec<_>>>()
                })
            })
            .collect();
        let mut encoded = Vec::with_capacity(blocks.len());
        for worker in workers {
            encoded.extend(worker.join().expect("compression thread panicked")?);
        }
        Ok(encoded)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_preserve_bytes_across_seeks_boundaries_and_short_reads() {
        for size in [4096, 16384, 65536] {
            let bytes: Vec<u8> = (0..size * 3 + 17)
                .map(|i| ((i * 31 + i / 71) % 251) as u8)
                .collect();
            let mut file = tempfile::tempfile().unwrap();
            let prefix = 79;
            file.seek(SeekFrom::Start(prefix)).unwrap();
            let length =
                encode(&mut bytes.as_slice(), &mut file, bytes.len() as u64, size).unwrap();
            file.seek(SeekFrom::Start(prefix)).unwrap();
            let mut reader = Blocks::open(&mut file, prefix, length, bytes.len() as u64).unwrap();
            for start in [size * 2 + 3, size - 1, 0, size * 3 + 16, size, 17] {
                let start = start as usize;
                let mut output = vec![0; (bytes.len() - start).min(size as usize + 13)];
                let mut read = 0;
                while read < output.len() {
                    let count = reader
                        .read(&mut file, (start + read) as u64, &mut output[read..])
                        .unwrap();
                    assert!(count > 0);
                    read += count;
                }
                assert_eq!(output, bytes[start..start + output.len()]);
            }
            assert_eq!(
                reader
                    .read(&mut file, bytes.len() as u64, &mut [0; 3])
                    .unwrap(),
                0
            );
            assert_eq!(reader.read(&mut file, 0, &mut []).unwrap(), 0);
        }
    }

    #[test]
    fn corrupt_block_tables_and_frames_fail_before_returning_data() {
        let data = vec![42; 8200];
        let mut file = tempfile::tempfile().unwrap();
        let length = encode(&mut data.as_slice(), &mut file, data.len() as u64, 4096).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        for (offset, replacement) in [(8, 0u32), (12, u32::MAX), (16, 0), (16, u32::MAX)] {
            let mut damaged = bytes.clone();
            damaged[offset..offset + 4].copy_from_slice(&replacement.to_le_bytes());
            file.seek(SeekFrom::Start(0)).unwrap();
            file.write_all(&damaged).unwrap();
            file.seek(SeekFrom::Start(0)).unwrap();
            assert!(Blocks::open(&mut file, 0, length, data.len() as u64).is_err());
        }
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&bytes).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut blocks = Blocks::open(&mut file, 0, length, data.len() as u64).unwrap();
        assert_eq!(blocks.read(&mut file, 0, &mut [0; 1]).unwrap(), 1);
        let second = &mut blocks.entries[1];
        let invalid = vec![0; second.length];
        file.seek(SeekFrom::Start(second.offset)).unwrap();
        file.write_all(&invalid).unwrap();
        assert!(blocks.read(&mut file, 4096, &mut [0; 1]).is_err());
        // Even a forged compressed checksum cannot bypass the bounded decoder.
        blocks.entries[1].sha256 = Sha256::digest(&invalid).into();
        assert!(blocks.read(&mut file, 4096, &mut [0; 1]).is_err());
        assert!(blocks.cached.is_none());
        let mut valid = [0; 1];
        blocks.read(&mut file, 0, &mut valid).unwrap();
        assert_eq!(valid, [42]);
    }
}
