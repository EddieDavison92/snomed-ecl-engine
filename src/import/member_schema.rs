use super::{active, date, id, rows};
use crate::store::{MemberColumn as C, MemberStore, NumericStore, TextColumn};
use anyhow::{ensure, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;
use zip::ZipArchive;

const DESCRIPTOR: u64 = 900000000000456007;

#[derive(Default)]
pub(super) struct Schemas(BTreeMap<u64, BTreeMap<u32, u64>>);

impl Schemas {
    pub fn read(
        archive: &mut ZipArchive<BufReader<File>>,
        prior: Option<&MemberStore>,
        edition: u32,
    ) -> Result<Self> {
        let mut schemas = Self::default();
        if let Some(table) = prior.map(|p| p.get(DESCRIPTOR)).transpose()?.flatten() {
            let (
                Some(C::Id(refsets)),
                Some(C::Id(types)),
                Some(C::Integer(orders)),
                Some(C::Boolean(active)),
            ) = (
                table.column("referencedComponentId"),
                table.column("attributeType"),
                table.column("attributeOrder"),
                table.column("active"),
            )
            else {
                anyhow::bail!("Invalid base refset descriptor schema");
            };
            for i in 0..table.len() {
                if active[i] != 0 {
                    schemas.insert(refsets[i], u32::try_from(orders[i])?, types[i])?;
                }
            }
        }
        let names: Vec<_> = archive
            .file_names()
            .filter(|n| {
                n.contains("/Snapshot/") && n.contains("RefsetDescriptor") && n.ends_with(".txt")
            })
            .map(str::to_owned)
            .collect();
        for name in names {
            rows(
                archive,
                &name,
                &[
                    "id",
                    "effectiveTime",
                    "active",
                    "moduleId",
                    "refsetId",
                    "referencedComponentId",
                    "attributeDescription",
                    "attributeType",
                    "attributeOrder",
                ],
                |r| {
                    ensure!(
                        id(r[4])? == DESCRIPTOR && date(r[1])? <= edition,
                        "Invalid refset descriptor metadata"
                    );
                    if active(r[2])? {
                        schemas.insert(id(r[5])?, r[8].parse()?, id(r[7])?)?;
                    }
                    Ok(())
                },
            )?;
        }
        for schema in schemas.0.values() {
            ensure!(
                schema.keys().copied().eq(0..schema.len() as u32),
                "Non-contiguous refset descriptor positions"
            );
        }
        Ok(schemas)
    }

    fn insert(&mut self, refset: u64, position: u32, kind: u64) -> Result<()> {
        ensure!(
            self.0
                .entry(refset)
                .or_default()
                .insert(position, kind)
                .is_none(),
            "Duplicate active refset descriptor position"
        );
        Ok(())
    }

    pub fn resolve(
        &self,
        refset: u64,
        store: &NumericStore,
    ) -> Result<Option<&BTreeMap<u32, u64>>> {
        let mut level = BTreeSet::from([refset]);
        let mut seen = BTreeSet::new();
        while !level.is_empty() {
            let mut found = None;
            let mut next = BTreeSet::new();
            for id in level {
                if !seen.insert(id) {
                    continue;
                }
                if let Some(schema) = self.0.get(&id) {
                    ensure!(
                        found.is_none_or(|other| other == schema),
                        "Conflicting nearest refset descriptors"
                    );
                    found = Some(schema);
                } else if let Some(o) = store.ordinal(id) {
                    next.extend(store.parents.get(o).iter().map(|&p| store.ids[p as usize]));
                }
            }
            if found.is_some() {
                return Ok(found);
            }
            level = next;
        }
        Ok(None)
    }
}

pub(super) fn column(
    kind: Option<u64>,
    physical: u8,
    field: &str,
    refset: u64,
    store: &NumericStore,
) -> Result<C> {
    ensure!(
        matches!(physical, b'c' | b'i' | b's'),
        "Unknown RF2 field type"
    );
    // MRCM represents its Boolean grouped field as an unsigned integer.
    if field.eq_ignore_ascii_case("grouped") && ancestors(refset, store).contains(&723604009) {
        return Ok(C::Boolean(vec![]));
    }
    if let Some(kind) = kind {
        let types = ancestors(kind, store);
        if types.contains(&1119403002) {
            return Ok(C::Number(TextColumn::default()));
        }
        if types.contains(&900000000000475002) {
            return Ok(C::Time(vec![]));
        }
        if types.contains(&900000000000474003) || types.contains(&900000000000464001) {
            return Ok(C::Uuid(vec![]));
        }
        if types.contains(&900000000000476001) {
            return Ok(C::Integer(vec![]));
        }
        if types.contains(&1119460002) {
            return Ok(C::Text(TextColumn::default()));
        }
        if types.contains(&900000000000460005) {
            ensure!(
                physical == b'c',
                "Component descriptor conflicts with RF2 representation"
            );
            return Ok(C::Id(vec![]));
        }
        ensure!(
            types.contains(&900000000000465000),
            "Unknown refset descriptor datatype"
        );
    } else if refset == 900000000000534007
        && matches!(field, "sourceEffectiveTime" | "targetEffectiveTime")
    {
        return Ok(C::Time(vec![]));
    }
    Ok(match physical {
        b'c' => C::Id(vec![]),
        b'i' => C::Integer(vec![]),
        _ => C::Text(TextColumn::default()),
    })
}

fn ancestors(id: u64, store: &NumericStore) -> BTreeSet<u64> {
    let mut seen = BTreeSet::new();
    let mut todo = vec![id];
    while let Some(id) = todo.pop() {
        if seen.insert(id) {
            if let Some(o) = store.ordinal(id) {
                todo.extend(store.parents.get(o).iter().map(|&p| store.ids[p as usize]));
            }
        }
    }
    seen
}

pub(super) fn push_number(column: &mut TextColumn, text: &str) -> Result<()> {
    crate::decimal::Decimal::parse(text).context("Invalid decimal member field")?;
    column.push(text)
}
