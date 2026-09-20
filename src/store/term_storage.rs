//! Description text stays on disk; readers retain one bounded window.
use super::*;
use std::sync::Mutex;

const WINDOW: u64 = 64 * 1024;

#[derive(Debug)]
pub(super) enum TermStorage {
    Owned(String),
    Stored {
        section: Section,
        start: u64,
        length: usize,
        window: Box<Mutex<Window>>,
    },
}
impl Default for TermStorage {
    fn default() -> Self {
        Self::Owned(String::new())
    }
}
#[derive(Debug)]
pub(super) struct Window {
    reader: SectionReader,
    start: u64,
    bytes: Vec<u8>,
}
impl TermStorage {
    pub fn stored(section: Section, start: u64, length: usize) -> Result<Self> {
        ensure!(
            start.checked_add(length as u64) == Some(section.length),
            "Invalid term section bounds"
        );
        let reader = section.reader()?;
        Ok(Self::Stored {
            section,
            start,
            length,
            window: Box::new(Mutex::new(Window {
                reader,
                start: u64::MAX,
                bytes: Vec::new(),
            })),
        })
    }
    pub fn len(&self) -> usize {
        match self {
            Self::Owned(text) => text.len(),
            Self::Stored { length, .. } => *length,
        }
    }
    pub fn with_range<T>(
        &self,
        from: usize,
        to: usize,
        visit: impl FnOnce(&str) -> T,
    ) -> Result<T> {
        ensure!(from <= to && to <= self.len(), "Invalid term offsets");
        match self {
            Self::Owned(text) => Ok(visit(
                text.get(from..to).context("Invalid term UTF-8 offset")?,
            )),
            Self::Stored {
                section,
                start,
                window,
                ..
            } => {
                let from = start + from as u64;
                let to = start + to as u64;
                let mut window = window
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Description text reader was poisoned"))?;
                if window.start > from
                    || window
                        .start
                        .checked_add(window.bytes.len() as u64)
                        .is_none_or(|end| to > end)
                {
                    // Invalidating first prevents reuse after a failed read or checksum check.
                    window.start = u64::MAX;
                    let low = from / WINDOW * WINDOW;
                    let high = (low + WINDOW).max(to).min(section.length);
                    window.reader.seek(SeekFrom::Start(low))?;
                    window.bytes.resize(usize::try_from(high - low)?, 0);
                    let Window { reader, bytes, .. } = &mut *window;
                    reader.read_exact(bytes)?;
                    window.start = low;
                }
                let text = std::str::from_utf8(
                    &window.bytes[(from - window.start) as usize..(to - window.start) as usize],
                )?;
                Ok(visit(text))
            }
        }
    }
    pub fn validate(&self, offsets: &[u32]) -> Result<()> {
        match self {
            Self::Owned(text) => ensure!(
                offsets.iter().all(|&v| text.is_char_boundary(v as usize)),
                "Invalid description UTF-8 offset"
            ),
            Self::Stored {
                section,
                start,
                length,
                ..
            } => {
                let mut reader = section.reader()?;
                reader.seek(SeekFrom::Start(*start))?;
                validate_utf8(&mut reader.take(*length as u64), offsets)?;
            }
        }
        Ok(())
    }
    pub fn write(&self, output: &mut impl Write) -> Result<()> {
        put_u64(output, self.len() as u64)?;
        match self {
            Self::Owned(text) => output.write_all(text.as_bytes())?,
            Self::Stored {
                section,
                start,
                length,
                ..
            } => {
                let mut reader = section.reader()?;
                reader.seek(SeekFrom::Start(*start))?;
                ensure!(
                    std::io::copy(&mut reader.take(*length as u64), output)? == *length as u64,
                    "Truncated term text"
                );
            }
        }
        Ok(())
    }
}

fn validate_utf8(reader: &mut impl Read, offsets: &[u32]) -> Result<()> {
    let mut buffer = vec![0; WINDOW as usize + 4];
    let (mut carry, mut processed, mut boundary) = (0, 0usize, 0usize);
    loop {
        let read = reader.read(&mut buffer[carry..])?;
        if read == 0 {
            ensure!(carry == 0, "Truncated description UTF-8");
            break;
        }
        let used = carry + read;
        let valid = match std::str::from_utf8(&buffer[..used]) {
            Ok(_) => used,
            Err(error) => {
                ensure!(error.error_len().is_none(), "Invalid description UTF-8");
                error.valid_up_to()
            }
        };
        let text = std::str::from_utf8(&buffer[..valid])?;
        while boundary < offsets.len() && offsets[boundary] as usize <= processed + valid {
            ensure!(
                (offsets[boundary] as usize) >= processed
                    && text.is_char_boundary(offsets[boundary] as usize - processed),
                "Invalid description UTF-8 offset"
            );
            boundary += 1;
        }
        processed += valid;
        carry = used - valid;
        buffer.copy_within(valid..used, 0);
    }
    // An empty text buffer still has its initial/final boundary at zero.
    if processed == 0 && offsets == [0] {
        boundary = 1;
    }
    ensure!(
        boundary == offsets.len() && offsets.last().is_some_and(|&v| v as usize == processed),
        "Description text length differs"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf8_validation_preserves_boundaries_across_short_reads() {
        let text = format!("{}é🦀{}", "x".repeat(WINDOW as usize - 1), "z".repeat(30));
        let offsets = [0, WINDOW as u32 - 1, WINDOW as u32 + 1, text.len() as u32];
        validate_utf8(&mut text.as_bytes(), &offsets).unwrap();
        assert!(
            validate_utf8(&mut text.as_bytes(), &[0, WINDOW as u32, text.len() as u32]).is_err()
        );
        assert!(validate_utf8(
            &mut &text.as_bytes()[..text.len() - 31],
            &[0, (text.len() - 31) as u32]
        )
        .is_err());
        assert!(validate_utf8(&mut &[b'a', 255][..], &[0, 2]).is_err());
        validate_utf8(&mut &[][..], &[0]).unwrap();
    }
}
