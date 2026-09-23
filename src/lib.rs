//! Evaluate SNOMED CT Expression Constraint Language against an index built from
//! an RF2 Snapshot, inside your own process.
//!
//! [`store::NumericStore`] opens an index, [`ecl::parse`] parses an expression
//! and [`eval::evaluate`] returns the matching concepts as ordinals, which
//! resolve to SCTIDs through `store.ids`. Open the store once and reuse it.
//!
//! ```no_run
//! use snomed_ecl_engine::{ecl, eval, store::NumericStore};
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let store = NumericStore::open(Path::new("uk.ecl"))?;
//! let expression = ecl::parse("<< 64572001 |Disease|")?;
//! let ordinals = eval::evaluate(&store, &expression)?;
//! let codes: Vec<u64> = ordinals.iter().map(|&o| store.ids[o as usize]).collect();
//! # Ok(())
//! # }
//! ```
//!
//! Features: `import` (default) builds indexes from RF2 archives; `unicode`
//! links ICU for term matching in description filters. Without `import`, the
//! crate can still open, query, pack and verify existing indexes.
#![deny(unsafe_code)]

pub mod config;
pub mod decimal;
pub mod detail;
pub mod ecl;
pub mod eval;
#[cfg(feature = "import")]
pub mod import;
pub mod store;
pub mod text;
