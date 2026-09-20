use super::*;
use crate::store::{Identifier, IdentifierIndex};

pub(super) fn read(
    archive: &mut ZipArchive<BufReader<File>>,
    lookup: &HashMap<u64, u32>,
    edition_date: u32,
) -> Result<Vec<Identifier>> {
    let names: Vec<_> = archive
        .file_names()
        .filter(|n| {
            n.contains("/Snapshot/")
                && n.rsplit('/')
                    .next()
                    .is_some_and(|n| n.starts_with("sct2_Identifier_") && n.ends_with(".txt"))
        })
        .map(str::to_owned)
        .collect();
    let mut identifiers = Vec::new();
    for name in names {
        rows(
            archive,
            &name,
            &[
                "alternateIdentifier",
                "effectiveTime",
                "active",
                "moduleId",
                "identifierSchemeId",
                "referencedComponentId",
            ],
            |r| {
                let effective = date(r[1])?;
                ensure!(
                    effective <= edition_date,
                    "Identifier is newer than edition"
                );
                let enabled = active(r[2])?;
                let module = id(r[3])?;
                let scheme = id(r[4])?;
                let reference = id(r[5])?;
                ensure!(
                    lookup.contains_key(&module) && lookup.contains_key(&scheme),
                    "Identifier module or scheme is absent"
                );
                if enabled && matches!((reference / 10) % 100, 0 | 10) {
                    ensure!(
                        lookup.contains_key(&reference),
                        "Identifier references a missing concept"
                    );
                }
                identifiers.push(Identifier {
                    scheme,
                    code: r[0].into(),
                    referenced_component: reference,
                    module,
                    effective_time: effective,
                    active: enabled,
                });
                Ok(())
            },
        )?;
    }
    Ok(IdentifierIndex::build(identifiers)?.rows)
}
