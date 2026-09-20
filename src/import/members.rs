use super::{active, date, id, rows};
use crate::store::{parse_uuid, MemberColumn, MemberManifest, MemberTable, TextColumn};
use anyhow::{ensure, Context, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use zip::ZipArchive;

/// File column types supply the RF2 representation; known time/Boolean fields keep their semantics.
pub(super) fn build(
    archive: &mut ZipArchive<BufReader<File>>,
    concepts: &HashMap<u64, u32>,
    edition_date: u32,
    directory: &Path,
    prior: Option<&crate::store::MemberStore>,
) -> Result<Vec<MemberManifest>> {
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
        let mut tables = BTreeMap::new();
        rows(archive, &name, &fields, |r| {
            let effective = date(r[1])?;
            ensure!(
                effective <= edition_date,
                "Refset member is newer than edition"
            );
            let enabled = active(r[2])?;
            let reference = id(r[5])?;
            if !matches!((reference / 10) % 100, 0 | 10) {
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
            let uuid = parse_uuid(r[0])?;
            ensure!(seen_ids.insert(uuid), "Duplicate Snapshot member UUID");
            let table = tables.entry(refset).or_insert_with(|| {
                let mut columns = vec![
                    MemberColumn::Uuid(vec![]),
                    MemberColumn::Time(vec![]),
                    MemberColumn::Boolean(vec![]),
                    MemberColumn::Id(vec![]),
                    MemberColumn::Id(vec![]),
                    MemberColumn::Id(vec![]),
                ];
                for (field, kind) in fields[6..].iter().zip(&types) {
                    columns.push(if field.ends_with("EffectiveTime") {
                        MemberColumn::Time(vec![])
                    } else if *field == "grouped" {
                        MemberColumn::Boolean(vec![])
                    } else {
                        match kind {
                            b'c' => MemberColumn::Id(vec![]),
                            b'i' => MemberColumn::Integer(vec![]),
                            _ => MemberColumn::Text(TextColumn::default()),
                        }
                    });
                }
                MemberTable {
                    refset,
                    names: fields.iter().map(|s| (*s).into()).collect(),
                    columns,
                }
            });
            for (i, column) in table.columns.iter_mut().enumerate() {
                match column {
                    MemberColumn::Uuid(v) => v.push(uuid),
                    MemberColumn::Time(v) => v.push(if i == 1 { effective } else { date(r[i])? }),
                    MemberColumn::Boolean(v) => {
                        v.push(u8::from(if i == 2 { enabled } else { active(r[i])? }))
                    }
                    MemberColumn::Id(v) => v.push(id(r[i])?),
                    MemberColumn::Integer(v) => {
                        v.push(r[i].parse().context("Invalid integer member field")?)
                    }
                    MemberColumn::Text(v) => {
                        ensure!(types[i - 6] == b's', "Unknown RF2 field type");
                        v.push(r[i])?;
                    }
                }
            }
            Ok(())
        })?;
        for (_, mut table) in tables {
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
