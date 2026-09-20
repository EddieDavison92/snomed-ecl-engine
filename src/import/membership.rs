use super::{active, date, id, rows};
use crate::store::{MembershipIndex, NumericStore};
use anyhow::{ensure, Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use zip::ZipArchive;

const CONCEPT_TYPE: u64 = 900000000000461009;
const DESCRIPTION_TYPE: u64 = 900000000000462002;
const RELATIONSHIP_TYPE: u64 = 900000000000463007;

/// Component kinds referenced by a reference set across every Snapshot row.
#[derive(Default)]
struct Kinds {
    concept_rows: u64,
    non_concept_rows: u64,
}

pub(super) fn read(
    archive: &mut ZipArchive<BufReader<File>>,
    concepts: &HashMap<u64, u32>,
    store: &NumericStore,
    edition_date: u32,
) -> Result<(MembershipIndex, u64, usize)> {
    let schemas = super::member_schema::Schemas::read(archive, None, edition_date)?;
    let mut names: Vec<_> = archive
        .file_names()
        .filter(|name| {
            name.contains("/Snapshot/")
                && name.ends_with(".txt")
                && name
                    .rsplit('/')
                    .next()
                    .is_some_and(|file| file.contains("Refset"))
        })
        .map(str::to_owned)
        .collect();
    names.sort();
    ensure!(!names.is_empty(), "No Snapshot refset files found");
    let mut pairs = Vec::new();
    let mut non_concept_rows = 0;
    let mut kinds: BTreeMap<u64, Kinds> = BTreeMap::new();
    for name in &names {
        let mut header = String::new();
        BufReader::new(archive.by_name(name)?).read_line(&mut header)?;
        let columns: Vec<_> = header
            .trim_start_matches('\u{feff}')
            .trim_end_matches(['\r', '\n'])
            .split('\t')
            .collect();
        ensure!(
            columns.starts_with(&[
                "id",
                "effectiveTime",
                "active",
                "moduleId",
                "refsetId",
                "referencedComponentId"
            ]),
            "Unexpected refset Snapshot header for {name}"
        );
        rows(archive, name, &columns, |row| {
            ensure!(
                date(row[1])? <= edition_date,
                "Refset member is newer than edition"
            );
            let enabled = active(row[2])?;
            let refset_id = id(row[4])?;
            let refset = *concepts
                .get(&refset_id)
                .context("Referenced refset concept is missing from package")?;
            let referenced = id(row[5])?;
            let kind = kinds.entry(refset_id).or_default();
            // RF2 SCTID partition 00/10 identifies concepts; 01/11 descriptions and 02/12 relationships.
            // Never turn a language-refset description member into its owning concept.
            match (referenced / 10) % 100 {
                0 | 10 => {
                    kind.concept_rows += 1;
                    if enabled {
                        let member = *concepts
                            .get(&referenced)
                            .context("Refset references a missing concept")?;
                        pairs.push((refset, member));
                    }
                }
                1 | 11 | 2 | 12 => {
                    kind.non_concept_rows += 1;
                    if enabled {
                        non_concept_rows += 1;
                    }
                }
                _ => anyhow::bail!("Unsupported referenced component identifier partition"),
            }
            Ok(())
        })?;
    }
    let mut index = MembershipIndex::build(concepts.len(), pairs)?;
    let (concept_refsets, non_concept_refsets) = classify(&kinds, &schemas, store)?;
    index.concept_refsets = Some(concept_refsets);
    index.non_concept_refsets = Some(non_concept_refsets);
    Ok((index, non_concept_rows, names.len()))
}

/// Section 6.1 confines memberOf to reference sets whose referenced components are concepts.
/// The RF2 descriptor's referencedComponentId type is the declaration; a generic component
/// type or a missing descriptor leaves the decision to the rows of every status.
fn classify(
    kinds: &BTreeMap<u64, Kinds>,
    schemas: &super::member_schema::Schemas,
    store: &NumericStore,
) -> Result<(Vec<u64>, Vec<u64>)> {
    let mut concept = Vec::new();
    let mut non_concept = Vec::new();
    let mut refsets: Vec<u64> = kinds.keys().copied().collect();
    refsets.extend(schemas.declared_refsets());
    refsets.sort_unstable();
    refsets.dedup();
    for refset in refsets {
        let declared = schemas
            .resolve(refset, store)?
            .and_then(|schema| schema.get(&0).copied())
            .map(|kind| super::member_schema::ancestors(kind, store));
        let rows = kinds.get(&refset);
        let in_domain = match &declared {
            Some(types)
                if types.contains(&DESCRIPTION_TYPE) || types.contains(&RELATIONSHIP_TYPE) =>
            {
                false
            }
            Some(types) if types.contains(&CONCEPT_TYPE) => true,
            _ => rows.is_none_or(|k| k.concept_rows > 0 || k.non_concept_rows == 0),
        };
        if in_domain {
            concept.push(refset);
        } else {
            non_concept.push(refset);
        }
    }
    Ok((concept, non_concept))
}
