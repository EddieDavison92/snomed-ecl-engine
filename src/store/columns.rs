//! Adaptive in-memory dictionaries. Component bytes remain format-compatible.
use super::*;
use std::collections::HashMap;

#[derive(Debug)]
pub(super) enum Codes {
    Byte(Box<[u8]>),
    Short(Box<[u16]>),
    Wide(Box<[u32]>),
}
impl Default for Codes {
    fn default() -> Self {
        Self::Byte(Box::default())
    }
}
impl Codes {
    pub fn new(values: Vec<u32>) -> Self {
        match values.iter().max().copied().unwrap_or(0) {
            0..=255 => Self::Byte(values.into_iter().map(|v| v as u8).collect()),
            256..=65535 => Self::Short(values.into_iter().map(|v| v as u16).collect()),
            _ => Self::Wide(values.into_boxed_slice()),
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Self::Byte(v) => v.len(),
            Self::Short(v) => v.len(),
            Self::Wide(v) => v.len(),
        }
    }
    pub fn get(&self, row: usize) -> u32 {
        match self {
            Self::Byte(v) => v[row] as u32,
            Self::Short(v) => v[row] as u32,
            Self::Wide(v) => v[row],
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Column {
    dictionary: Vec<u32>,
    codes: Codes,
}
impl Column {
    pub fn new(mut values: Vec<u32>) -> Self {
        let mut dictionary = Vec::new();
        let mut lookup = HashMap::new();
        for row in 0..values.len() {
            if dictionary.len() == 65536 && !lookup.contains_key(&values[row]) {
                for value in &mut values[..row] {
                    *value = dictionary[*value as usize];
                }
                return Self {
                    dictionary: Vec::new(),
                    codes: Codes::Wide(values.into_boxed_slice()),
                };
            }
            let value = values[row];
            values[row] = *lookup.entry(value).or_insert_with(|| {
                dictionary.push(value);
                dictionary.len() as u32 - 1
            });
        }
        let width = if dictionary.len() <= 256 { 1 } else { 2 };
        if dictionary.len() * 4 + values.len() * width >= values.len() * 4 {
            for value in &mut values {
                *value = dictionary[*value as usize];
            }
            Self {
                dictionary: Vec::new(),
                codes: Codes::Wide(values.into_boxed_slice()),
            }
        } else {
            Self {
                dictionary,
                codes: Codes::new(values),
            }
        }
    }
    pub fn len(&self) -> usize {
        self.codes.len()
    }
    pub fn get(&self, row: usize) -> u32 {
        let code = self.codes.get(row);
        if self.dictionary.is_empty() {
            code
        } else {
            self.dictionary[code as usize]
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.len()).map(|i| self.get(i))
    }
    pub fn write(&self, out: &mut impl Write) -> Result<()> {
        put_u64(out, self.len() as u64)?;
        for value in self.iter() {
            put_u32(out, value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub(super) struct RowSets {
    offsets: Vec<u32>,
    values: Vec<u32>,
    selectors: Option<Codes>,
    rows: usize,
    entries: usize,
}
impl RowSets {
    pub fn read(input: &mut Input, rows: usize) -> Result<Self> {
        let offsets = input.u32s()?;
        ensure!(
            offsets.len() == rows + 1,
            "Description dialect row count differs"
        );
        let entries = input.count(4)?;
        Self::decode(offsets, entries, || input.u32())
    }

    fn decode(
        offsets: Vec<u32>,
        entries: usize,
        mut read: impl FnMut() -> Result<u32>,
    ) -> Result<Self> {
        let rows = offsets
            .len()
            .checked_sub(1)
            .context("Missing dialect offsets")?;
        validate_offsets(&offsets, rows, entries)?;
        ensure!(
            offsets.iter().all(|v| v % 2 == 0),
            "Invalid language member offset"
        );
        let mut lookup = HashMap::new();
        let mut unique_offsets = vec![0u32];
        let mut unique_values = Vec::new();
        let mut selectors = Vec::with_capacity(rows);
        let mut buffer = Vec::new();
        for bounds in offsets.windows(2) {
            buffer.clear();
            for _ in bounds[0]..bounds[1] {
                buffer.push(read()?);
            }
            let code = if let Some(&code) = lookup.get(buffer.as_slice()) {
                code
            } else {
                // Fall back before unusual editions build an unbounded dictionary.
                if lookup.len() == 4096 || unique_values.len() + buffer.len() > 1024 * 1024 {
                    let mut values = Vec::with_capacity(entries);
                    for &code in &selectors {
                        values.extend_from_slice(
                            &unique_values[unique_offsets[code as usize] as usize
                                ..unique_offsets[code as usize + 1] as usize],
                        );
                    }
                    values.extend_from_slice(&buffer);
                    while values.len() < entries {
                        values.push(read()?);
                    }
                    return Ok(Self {
                        offsets,
                        values,
                        selectors: None,
                        rows,
                        entries,
                    });
                }
                let code = lookup.len() as u32;
                unique_values.extend_from_slice(&buffer);
                unique_offsets.push(u32::try_from(unique_values.len())?);
                lookup.insert(buffer.clone(), code);
                code
            };
            selectors.push(code);
        }
        Ok(Self {
            offsets: unique_offsets,
            values: unique_values,
            selectors: Some(Codes::new(selectors)),
            rows,
            entries,
        })
    }

    pub fn new(offsets: Vec<u32>, values: Vec<u32>, rows: usize) -> Result<Self> {
        validate_offsets(&offsets, rows, values.len())?;
        ensure!(
            offsets.iter().all(|v| v % 2 == 0),
            "Invalid language member offset"
        );
        let entries = values.len();
        let mut lookup = HashMap::new();
        let mut ranges = Vec::new();
        let mut selectors = Vec::with_capacity(rows);
        for bounds in offsets.windows(2) {
            let slice = &values[bounds[0] as usize..bounds[1] as usize];
            if ranges.len() == 4096 && !lookup.contains_key(slice) {
                return Ok(Self {
                    offsets,
                    values,
                    selectors: None,
                    rows,
                    entries,
                });
            }
            let code = *lookup.entry(slice).or_insert_with(|| {
                ranges.push((bounds[0], bounds[1]));
                ranges.len() as u32 - 1
            });
            selectors.push(code);
        }
        let mut unique_offsets = vec![0];
        let mut unique_values = Vec::new();
        for (start, end) in ranges {
            unique_values.extend_from_slice(&values[start as usize..end as usize]);
            unique_offsets.push(u32::try_from(unique_values.len())?);
        }
        drop(lookup);
        if (unique_offsets.len() + unique_values.len()) * 4 + rows * 2
            >= (offsets.len() + values.len()) * 4
        {
            Ok(Self {
                offsets,
                values,
                selectors: None,
                rows,
                entries,
            })
        } else {
            Ok(Self {
                offsets: unique_offsets,
                values: unique_values,
                selectors: Some(Codes::new(selectors)),
                rows,
                entries,
            })
        }
    }
    pub fn entries(&self) -> usize {
        self.entries
    }
    pub fn get(&self, row: usize) -> &[u32] {
        let index = self.selectors.as_ref().map_or(row, |s| s.get(row) as usize);
        &self.values[self.offsets[index] as usize..self.offsets[index + 1] as usize]
    }
    pub fn validate(&self, count: usize, rows: usize) -> Result<()> {
        ensure!(rows == self.rows, "Description dialect row count differs");
        ensure!(
            self.values.iter().all(|&v| (v as usize) < count),
            "Invalid description dialect ordinal"
        );
        for bounds in self.offsets.windows(2) {
            let pairs = &self.values[bounds[0] as usize..bounds[1] as usize];
            ensure!(
                pairs
                    .chunks_exact(2)
                    .zip(pairs.chunks_exact(2).skip(1))
                    .all(|(left, right)| left < right),
                "Unordered language members"
            );
        }
        Ok(())
    }
    pub fn write(&self, out: &mut impl Write) -> Result<()> {
        put_u64(out, self.rows as u64 + 1)?;
        let mut offset = 0u32;
        put_u32(out, 0)?;
        for row in 0..self.rows {
            offset = offset
                .checked_add(u32::try_from(self.get(row).len())?)
                .context("Dialect size overflow")?;
            put_u32(out, offset)?;
        }
        put_u64(out, self.entries as u64)?;
        for row in 0..self.rows {
            for &value in self.get(row) {
                put_u32(out, value)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adaptive_columns_preserve_values_and_expand_on_disk() {
        for distinct in [1, 255, 256, 257, 65536, 65537] {
            let raw: Vec<_> = (0..distinct * 3)
                .map(|i| (i % distinct) as u32 * 37)
                .collect();
            let compact = Column::new(raw.clone());
            assert_eq!(compact.iter().collect::<Vec<_>>(), raw);
            let mut encoded = Vec::new();
            compact.write(&mut encoded).unwrap();
            assert_eq!(encoded.len(), 8 + raw.len() * 4);
            for (i, bytes) in encoded[8..].chunks_exact(4).enumerate() {
                assert_eq!(u32::from_le_bytes(bytes.try_into().unwrap()), raw[i]);
            }
        }
    }
    #[test]
    fn row_dictionaries_preserve_empty_rows_pairs_and_large_variety() {
        for distinct in [3, 4097] {
            let rows = distinct * 3;
            let mut offsets = vec![0];
            let mut values = Vec::new();
            for i in 0..rows {
                if i % distinct != 0 {
                    values.extend([i as u32 % distinct as u32, 5000]);
                }
                offsets.push(values.len() as u32);
            }
            let sets = RowSets::new(offsets.clone(), values.clone(), rows).unwrap();
            let mut source = values.iter().copied();
            let decoded = RowSets::decode(offsets.clone(), values.len(), || {
                source.next().context("Truncated test rows")
            })
            .unwrap();
            assert!(source.next().is_none());
            sets.validate(6000, rows).unwrap();
            for row in 0..rows {
                assert_eq!(
                    sets.get(row),
                    &values[offsets[row] as usize..offsets[row + 1] as usize]
                );
                assert_eq!(sets.get(row), decoded.get(row));
            }
            let mut encoded = Vec::new();
            sets.write(&mut encoded).unwrap();
            let mut original = Vec::new();
            put_u32s(&mut original, &offsets).unwrap();
            put_u32s(&mut original, &values).unwrap();
            assert_eq!(encoded, original);
        }
        assert!(RowSets::new(vec![0, 1], vec![0], 1).is_err());
    }
}
