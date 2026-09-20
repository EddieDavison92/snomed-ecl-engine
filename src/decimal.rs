//! Exact decimal comparison without conversion to binary floating point.
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decimal {
    negative: bool,
    integer: String,
    fraction: String,
}
impl Decimal {
    pub fn parse(text: &str) -> Option<Self> {
        let negative = text.starts_with('-');
        let text = text.strip_prefix(['-', '+']).unwrap_or(text);
        let (integer, fraction) = text.split_once('.').unwrap_or((text, ""));
        if integer.is_empty()
            || !integer.bytes().all(|c| c.is_ascii_digit())
            || !fraction.bytes().all(|c| c.is_ascii_digit())
            || text.contains('.') && fraction.is_empty()
        {
            return None;
        }
        let integer = integer.trim_start_matches('0');
        let fraction = fraction.trim_end_matches('0');
        Some(Self {
            negative: negative && (!integer.is_empty() || !fraction.is_empty()),
            integer: integer.into(),
            fraction: fraction.into(),
        })
    }
}
impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.negative != other.negative {
            return if self.negative {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let magnitude = self
            .integer
            .len()
            .cmp(&other.integer.len())
            .then_with(|| self.integer.cmp(&other.integer))
            .then_with(|| {
                let length = self.fraction.len().max(other.fraction.len());
                self.fraction
                    .bytes()
                    .chain(std::iter::repeat(b'0'))
                    .take(length)
                    .cmp(
                        other
                            .fraction
                            .bytes()
                            .chain(std::iter::repeat(b'0'))
                            .take(length),
                    )
            });
        if self.negative {
            magnitude.reverse()
        } else {
            magnitude
        }
    }
}
impl std::fmt::Display for Decimal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.negative {
            f.write_str("-")?;
        }
        f.write_str(if self.integer.is_empty() {
            "0"
        } else {
            &self.integer
        })?;
        if !self.fraction.is_empty() {
            write!(f, ".{}", self.fraction)?;
        }
        Ok(())
    }
}
impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
