use super::{put_u32s, sha256, validate_offsets, IndexSource, Input};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

const MAGIC: &[u8; 8] = b"SNECLM01";

#[derive(Debug, Serialize, Deserialize)]
pub struct MembershipManifest {
    pub bytes: u64,
    pub sha256: String,
    pub refset_count: usize,
    pub concept_pairs: usize,
    pub active_non_concept_rows: u64,
    pub snapshot_files: usize,
    /// Reference sets inside the memberOf domain, as sorted SCTIDs: their descriptor declares
    /// a concept referencedComponentId, or a generic/missing declaration has concept rows
    /// of any status (or no rows). Absent in older indexes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concept_refsets: Option<Vec<u64>>,
    /// Reference sets outside that domain: their descriptor declares a description or
    /// relationship referencedComponentId, or, without a descriptor, every Snapshot row of
    /// any status references a non-concept. Absent in older indexes, which then cannot
    /// report memberOf over such sets as an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub non_concept_refsets: Option<Vec<u64>>,
}

/// Sparse refset keys and sorted, unique concept ordinals for each key.
/// Member metadata and non-concept projections require a separate semantic index.
#[derive(Debug, Default)]
pub struct MembershipIndex {
    pub refsets: Vec<u32>,
    pub offsets: Vec<u32>,
    pub members: Vec<u32>,
    /// Sorted SCTIDs of reference sets whose declared or observed referenced components
    /// include concepts. None when the index predates this metadata.
    pub concept_refsets: Option<Vec<u64>>,
    /// Sorted SCTIDs of reference sets whose referenced components are only descriptions or
    /// relationships. None when the index predates this metadata.
    pub non_concept_refsets: Option<Vec<u64>>,
}

impl MembershipIndex {
    pub fn build(count: usize, mut pairs: Vec<(u32, u32)>) -> Result<Self> {
        pairs.sort_unstable();
        pairs.dedup();
        ensure!(
            pairs.len() < u32::MAX as usize,
            "Membership index exceeds u32 capacity"
        );
        let mut index = Self {
            offsets: vec![0],
            ..Self::default()
        };
        for (refset, member) in pairs {
            if index.refsets.last() != Some(&refset) {
                if !index.refsets.is_empty() {
                    index.offsets.push(index.members.len() as u32);
                }
                index.refsets.push(refset);
            }
            index.members.push(member);
        }
        if !index.refsets.is_empty() {
            index.offsets.push(index.members.len() as u32);
        }
        index.validate(count)?;
        Ok(index)
    }

    pub fn get(&self, position: usize) -> &[u32] {
        &self.members[self.offsets[position] as usize..self.offsets[position + 1] as usize]
    }

    pub fn validate(&self, count: usize) -> Result<()> {
        validate_offsets(&self.offsets, self.refsets.len(), self.members.len())?;
        ensure!(
            self.concept_refsets.is_some() == self.non_concept_refsets.is_some(),
            "Reference set domain lists must be recorded together"
        );
        for refsets in [&self.concept_refsets, &self.non_concept_refsets]
            .into_iter()
            .flatten()
        {
            ensure!(
                refsets.windows(2).all(|w| w[0] < w[1])
                    && refsets
                        .iter()
                        .all(|&id| id > 0 && id < 1_000_000_000_000_000_000),
                "Invalid reference set domain list"
            );
        }
        if let (Some(concept), Some(other)) = (&self.concept_refsets, &self.non_concept_refsets) {
            ensure!(
                concept.iter().all(|id| other.binary_search(id).is_err()),
                "Reference set listed in both domain lists"
            );
        }
        ensure!(
            self.refsets.windows(2).all(|w| w[0] < w[1])
                && self
                    .refsets
                    .iter()
                    .chain(&self.members)
                    .all(|&v| (v as usize) < count),
            "Invalid refset or membership concept ordinal"
        );
        for position in 0..self.refsets.len() {
            ensure!(
                self.get(position).windows(2).all(|w| w[0] < w[1]),
                "Duplicate or unordered membership"
            );
        }
        Ok(())
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let mut out = BufWriter::new(File::create_new(path)?);
        out.write_all(MAGIC)?;
        put_u32s(&mut out, &self.refsets)?;
        put_u32s(&mut out, &self.offsets)?;
        put_u32s(&mut out, &self.members)?;
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(())
    }

    pub fn manifest(
        &self,
        path: &Path,
        active_non_concept_rows: u64,
        snapshot_files: usize,
    ) -> Result<MembershipManifest> {
        Ok(MembershipManifest {
            bytes: path.metadata()?.len(),
            sha256: sha256(path)?,
            refset_count: self.refsets.len(),
            concept_pairs: self.members.len(),
            active_non_concept_rows,
            snapshot_files,
            concept_refsets: self.concept_refsets.clone(),
            non_concept_refsets: self.non_concept_refsets.clone(),
        })
    }

    pub(super) fn open(
        source: &IndexSource,
        metadata: &MembershipManifest,
        count: usize,
    ) -> Result<Self> {
        let mut input = Input::open(&source.section("membership.bin")?, MAGIC)?;
        let index = Self {
            refsets: input.u32s()?,
            offsets: input.u32s()?,
            members: input.u32s()?,
            concept_refsets: metadata.concept_refsets.clone(),
            non_concept_refsets: metadata.non_concept_refsets.clone(),
        };
        ensure!(input.remaining == 0, "Trailing membership bytes");
        ensure!(
            index.refsets.len() == metadata.refset_count
                && index.members.len() == metadata.concept_pairs,
            "Manifest membership counts differ"
        );
        index.validate(count)?;
        Ok(index)
    }
}
