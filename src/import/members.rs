use super::{active, date, id, rows};
use crate::store::{
    is_concept_id, parse_uuid, MemberColumn, MemberManifest, MemberTable, NumericStore, TextColumn,
    METADATA,
};
use anyhow::{ensure, Context, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use zip::ZipArchive;

/// Descriptors supply field semantics; filename types provide a fallback.
pub(super) fn build(
    archive: &mut ZipArchive<BufReader<File>>,
    concepts: &HashMap<u64, u32>,
    store: &NumericStore,
    edition_date: u32,
    directory: &Path,
    prior: Option<&crate::store::MemberStore>,
) -> Result<Vec<MemberManifest>> {
    let schemas = super::member_schema::Schemas::read(archive, prior, edition_date)?;
    let mut names: Vec<_> = archive
        .file_names()
        .filter(|n| {
            n.contains("/Snapshot/")
                && n.ends_with(".txt")
                && n.rsplit('/').next().is_some_and(|n| n.contains("Refset"))
        })
        .map(str::to_owned)
        .collect();
    names.sort();
    let mut manifests = Vec::new();
    let mut seen_refsets = HashSet::new();
    let mut seen_ids = HashSet::new();
    for name in names {
        let mut header = String::new();
        BufReader::new(archive.by_name(&name)?).read_line(&mut header)?;
        let fields: Vec<_> = header
            .trim_start_matches('\u{feff}')
            .trim_end_matches(['\r', '\n'])
            .split('\t')
            .collect();
        ensure!(
            fields.starts_with(&[
                "id",
                "effectiveTime",
                "active",
                "moduleId",
                "refsetId",
                "referencedComponentId"
            ]),
            "Invalid refset header"
        );
        let file = name.rsplit('/').next().unwrap();
        let pattern = file
            .split_once('_')
            .context("Missing refset field types")?
            .1
            .split_once("Refset")
            .context("Missing refset type prefix")?
            .0;
        ensure!(
            pattern.len() == fields.len() - 6,
            "Refset filename and field count differ"
        );
        let types: Vec<_> = pattern.bytes().collect();
        let column_names: Vec<String> = fields
            .iter()
            .map(|name| name.chars().filter(|c| !c.is_whitespace()).collect())
            .collect();
        let mut tables = BTreeMap::new();
        rows(archive, &name, &fields, |r| {
            let effective = date(r[1])?;
            ensure!(
                effective <= edition_date,
                "Refset member is newer than edition"
            );
            let enabled = active(r[2])?;
            let reference = id(r[5])?;
            if !is_concept_id(reference) {
                return Ok(());
            }
            let refset = id(r[4])?;
            let module = id(r[3])?;
            ensure!(
                [refset, module].iter().all(|c| concepts.contains_key(c)),
                "Refset member references a missing module or refset"
            );
            // UK inactive map rows can retain references absent from the concept snapshot.
            // Preserve their fields for tuple queries; never invent a concept ordinal.
            ensure!(
                !enabled || concepts.contains_key(&reference),
                "Active refset member references a missing concept"
            );
            // The UUID is checked for duplicates and then dropped; see store::METADATA.
            ensure!(
                seen_ids.insert(parse_uuid(r[0])?),
                "Duplicate Snapshot member UUID"
            );
            if let std::collections::btree_map::Entry::Vacant(entry) = tables.entry(refset) {
                let mut columns = vec![
                    MemberColumn::Time(vec![]),
                    MemberColumn::Boolean(vec![]),
                    MemberColumn::Id(vec![]),
                    MemberColumn::Id(vec![]),
                ];
                let schema = schemas.resolve(refset, store)?;
                if let Some(schema) = schema {
                    ensure!(
                        schema.len() == fields.len() - 5,
                        "Refset descriptor and column count differ"
                    );
                }
                for (index, (field, kind)) in column_names[6..].iter().zip(&types).enumerate() {
                    columns.push(super::member_schema::column(
                        schema.and_then(|s| s.get(&(index as u32 + 1))).copied(),
                        *kind,
                        field,
                        refset,
                        store,
                    )?);
                }
                // Integer columns promoted to decimal text keep integer validation for every row.
                let integer_columns = vec![false; columns.len()];
                entry.insert((
                    MemberTable {
                        refset,
                        names: METADATA
                            .iter()
                            .map(|name| name.to_string())
                            .chain(column_names[6..].iter().cloned())
                            .collect(),
                        columns,
                    },
                    integer_columns,
                ));
            }
            let (table, integer_columns) = tables.get_mut(&refset).unwrap();
            for (column_index, column) in table.columns.iter_mut().enumerate() {
                // The RF2 position of this column: id and refsetId are not stored.
                let i = match column_index {
                    0..=2 => column_index + 1,
                    3 => 5,
                    _ => column_index + 2,
                };
                match column {
                    MemberColumn::Uuid(v) => v.push(parse_uuid(r[i])?),
                    MemberColumn::Time(v) => v.push(if i == 1 {
                        effective
                    } else if r[i].is_empty() {
                        0
                    } else {
                        date(r[i])?
                    }),
                    MemberColumn::Boolean(v) => {
                        v.push(u8::from(if i == 2 { enabled } else { active(r[i])? }))
                    }
                    MemberColumn::Id(v) => v.push(id(r[i])?),
                    MemberColumn::Integer(v) => {
                        check_integer(r[i])?;
                        match r[i].parse::<i64>() {
                            Ok(value) => v.push(value),
                            Err(_) => {
                                // Integers can exceed i64; keep the whole column exact as decimal text.
                                let mut text = TextColumn::default();
                                for earlier in v.iter() {
                                    text.push(&earlier.to_string())?;
                                }
                                text.push(r[i])?;
                                *column = MemberColumn::Number(text);
                                integer_columns[column_index] = true;
                            }
                        }
                    }
                    MemberColumn::Number(v) if integer_columns[column_index] => {
                        check_integer(r[i])?;
                        v.push(r[i])?;
                    }
                    MemberColumn::Number(v) => super::member_schema::push_number(v, r[i])?,
                    MemberColumn::Text(v) => {
                        ensure!(types[i - 6] == b's', "Unknown RF2 field type");
                        v.push(r[i])?;
                    }
                }
            }
            Ok(())
        })?;
        for (_, (mut table, _)) in tables {
            ensure!(
                seen_refsets.insert(table.refset),
                "Refset occurs in multiple Snapshot files"
            );
            if let Some(base) = prior.map(|p| p.get(table.refset)).transpose()?.flatten() {
                let mut merged = base.clone();
                merged.append(table)?;
                table = merged;
            }
            manifests.push(table.write(directory)?);
        }
    }
    manifests.sort_by_key(|m| m.refset);
    Ok(manifests)
}

/// RF2 integers are decimal digits with an optional leading minus sign.
fn check_integer(text: &str) -> Result<()> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    ensure!(
        !digits.is_empty()
            && digits.bytes().all(|b| b.is_ascii_digit())
            && (digits == "0" || !digits.starts_with('0'))
            && text != "-0",
        "Invalid integer member field"
    );
    Ok(())
}
