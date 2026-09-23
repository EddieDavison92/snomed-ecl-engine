//! Sorted lists written as varint deltas.
//!
//! Each list is strictly increasing, so it is stored as its first value and
//! then the gaps between neighbours, seven bits a byte. Gaps between concepts
//! that share a word or a reference set are mostly small, so a list costs one
//! or two bytes a value rather than four. Lists are decoded whole when their
//! section loads, so queries still read plain `u32` slices. A member table's
//! referenced components are sorted but repeat, so their gaps may be zero.
use anyhow::{ensure, Result};

/// Encodes the lists `values[offsets[i]..offsets[i + 1]]`, each strictly increasing.
pub(super) fn encode(offsets: &[u32], values: &[u32]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(values.len() * 2);
    for bounds in offsets.windows(2) {
        let mut previous = None;
        for &value in &values[bounds[0] as usize..bounds[1] as usize] {
            let gap = match previous {
                None => value,
                Some(previous) => {
                    ensure!(value > previous, "List is not strictly increasing");
                    value - previous
                }
            };
            previous = Some(value);
            let mut gap = gap;
            while gap >= 0x80 {
                out.push((gap as u8 & 0x7F) | 0x80);
                gap >>= 7;
            }
            out.push(gap as u8);
        }
    }
    Ok(out)
}

/// Decodes the lists `encode` wrote for `offsets`, checking every byte is used.
pub(super) fn decode(offsets: &[u32], bytes: &[u8]) -> Result<Vec<u32>> {
    let total = offsets.last().copied().unwrap_or(0) as usize;
    // Every value takes at least a byte, so a damaged offset cannot reserve more.
    let mut values = Vec::with_capacity(total.min(bytes.len()));
    let mut at = 0;
    for bounds in offsets.windows(2) {
        ensure!(bounds[0] <= bounds[1], "Invalid list offsets");
        let mut previous: Option<u32> = None;
        for _ in bounds[0]..bounds[1] {
            let mut gap = 0u64;
            let mut shift = 0;
            loop {
                let byte = *bytes
                    .get(at)
                    .ok_or_else(|| anyhow::anyhow!("Truncated list"))?;
                at += 1;
                gap |= u64::from(byte & 0x7F) << shift;
                if byte & 0x80 == 0 {
                    break;
                }
                shift += 7;
                ensure!(shift < 35, "Invalid list value");
            }
            let value = match previous {
                None => gap,
                Some(previous) => {
                    ensure!(gap > 0, "List is not strictly increasing");
                    u64::from(previous) + gap
                }
            };
            let value =
                u32::try_from(value).map_err(|_| anyhow::anyhow!("List value overflows"))?;
            values.push(value);
            previous = Some(value);
        }
    }
    ensure!(
        at == bytes.len() && values.len() == total,
        "List bytes differ from offsets"
    );
    Ok(values)
}

/// Encodes a non-decreasing list of `u64` as its first value and then the
/// gaps, which may be zero, seven bits a byte.
pub(super) fn encode_u64(values: &[u64]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(values.len() * 2);
    let mut previous = 0;
    for &value in values {
        ensure!(value >= previous, "List is not sorted");
        let mut gap = value - previous;
        previous = value;
        while gap >= 0x80 {
            out.push((gap as u8 & 0x7F) | 0x80);
            gap >>= 7;
        }
        out.push(gap as u8);
    }
    Ok(out)
}

/// Decodes `count` values that `encode_u64` wrote, checking every byte is used.
pub(super) fn decode_u64(count: usize, bytes: &[u8]) -> Result<Vec<u64>> {
    let mut values = Vec::with_capacity(count);
    let mut at = 0;
    let mut previous = 0u64;
    for _ in 0..count {
        let mut gap = 0u64;
        let mut shift = 0;
        loop {
            let byte = *bytes
                .get(at)
                .ok_or_else(|| anyhow::anyhow!("Truncated list"))?;
            at += 1;
            ensure!(
                shift < 64 && (shift < 63 || byte <= 1),
                "Invalid list value"
            );
            gap |= u64::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        previous = previous
            .checked_add(gap)
            .ok_or_else(|| anyhow::anyhow!("List value overflows"))?;
        values.push(previous);
    }
    ensure!(at == bytes.len(), "List bytes differ from count");
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_round_trip_and_reject_damage() {
        let offsets = [0, 3, 3, 6];
        let values = [0, 1, 300, 5, 70_000, u32::MAX];
        let bytes = encode(&offsets, &values).unwrap();
        assert_eq!(decode(&offsets, &bytes).unwrap(), values);
        assert!(bytes.len() < values.len() * 4);
        assert!(encode(&[0, 2], &[5, 5]).is_err(), "not strictly increasing");
        assert!(
            decode(&offsets, &bytes[..bytes.len() - 1]).is_err(),
            "truncated"
        );
        let mut longer = bytes.clone();
        longer.push(1);
        assert!(decode(&offsets, &longer).is_err(), "trailing bytes");
    }

    #[test]
    fn sorted_u64_lists_round_trip_and_reject_damage() {
        let values = [0, 0, 5, 5, 1_000_000_000_000_000_001, u64::MAX];
        let bytes = encode_u64(&values).unwrap();
        assert_eq!(decode_u64(values.len(), &bytes).unwrap(), values);
        assert!(encode_u64(&[2, 1]).is_err(), "not sorted");
        assert!(
            decode_u64(values.len(), &bytes[..bytes.len() - 1]).is_err(),
            "truncated"
        );
        assert!(
            decode_u64(values.len() - 1, &bytes).is_err(),
            "trailing bytes"
        );
        assert!(decode_u64(2, &[0xFF; 11]).is_err(), "value too long");
    }
}
