//! Experimental RF2 snapshot storage. Numeric queries never open the display file.
#![forbid(unsafe_code)]

pub mod decimal;
pub mod ecl;
pub mod eval;
#[cfg(feature = "import")]
pub mod import;
pub mod store;
