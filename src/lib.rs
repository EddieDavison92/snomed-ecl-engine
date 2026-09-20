//! Experimental RF2 snapshot storage. Numeric queries never open the display file.
#![deny(unsafe_code)]

pub mod config;
pub mod decimal;
pub mod ecl;
pub mod eval;
#[cfg(feature = "import")]
pub mod import;
pub mod store;
pub mod text;
