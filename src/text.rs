//! Unicode collation support for ECL text predicates.
#[cfg(feature = "unicode")]
#[allow(unsafe_code)]
mod icu;

#[cfg(feature = "unicode")]
pub use icu::Search;
#[cfg(feature = "unicode")]
mod terms;
#[cfg(feature = "unicode")]
pub use terms::Terms;

#[cfg(feature = "unicode")]
pub const COLLATION_VERSION: &str = concat!("ICU4C ", env!("SNOMED_ICU_VERSION"));

#[derive(Debug, PartialEq, Eq)]
pub enum TextError {
    InvalidInput,
    Icu(i32),
}
