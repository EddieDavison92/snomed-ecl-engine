use super::{active, date, id, rows};
use crate::store::MembershipIndex;
use anyhow::{ensure, Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use zip::ZipArchive;

pub(super) fn read(
    archive: &mut ZipArchive<BufReader<File>>,
    concepts: &HashMap<u64, u32>,
    edition_date: u32,
) -> Result<(MembershipIndex, u64, usize)> {
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
            if !active(row[2])? {
                return Ok(());
            }
            let refset = *concepts
                .get(&id(row[4])?)
                .context("Referenced refset concept is missing from package")?;
            let referenced = id(row[5])?;
            // RF2 SCTID partition 00/10 identifies concepts; 01/11 descriptions and 02/12 relationships.
            // Never turn a language-refset description member into its owning concept.
            match (referenced / 10) % 100 {
                0 | 10 => {
                    let member = *concepts
                        .get(&referenced)
                        .context("Refset references a missing concept")?;
                    pairs.push((refset, member));
                }
                1 | 11 | 2 | 12 => {
                    non_concept_rows += 1;
                }
                _ => anyhow::bail!("Unsupported referenced component identifier partition"),
            }
            Ok(())
        })?;
    }
    Ok((
        MembershipIndex::build(concepts.len(), pairs)?,
        non_concept_rows,
        names.len(),
    ))
}
