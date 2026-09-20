use super::*;
use crate::store::{Description, DescriptionIndex};

pub(super) fn read(
    archive: &mut ZipArchive<BufReader<File>>,
    lookup: &HashMap<u64, u32>,
    edition_date: u32,
) -> Result<Vec<Description>> {
    let resolve = |sctid| {
        lookup
            .get(&sctid)
            .copied()
            .context("Unknown description metadata concept")
    };
    let files: Vec<_> = archive
        .file_names()
        .filter(|n| {
            n.contains("/Snapshot/")
                && n.ends_with(".txt")
                && n.rsplit('/').next().is_some_and(|f| {
                    f.starts_with("sct2_Description_") || f.starts_with("sct2_TextDefinition_")
                })
        })
        .map(str::to_owned)
        .collect();
    let mut descriptions = Vec::new();
    let mut positions = HashMap::new();
    for file in files {
        rows(
            archive,
            &file,
            &[
                "id",
                "effectiveTime",
                "active",
                "moduleId",
                "conceptId",
                "languageCode",
                "typeId",
                "term",
                "caseSignificanceId",
            ],
            |r| {
                let description = id(r[0])?;
                ensure!(
                    positions.insert(description, descriptions.len()).is_none(),
                    "Duplicate description ID in Snapshot"
                );
                let effective_time = date(r[1])?;
                ensure!(
                    effective_time <= edition_date,
                    "Description is newer than edition"
                );
                let language: [u8; 2] = r[5]
                    .as_bytes()
                    .try_into()
                    .context("Expected two-letter RF2 language")?;
                ensure!(
                    language.iter().all(u8::is_ascii_lowercase),
                    "Expected lowercase RF2 language"
                );
                id(r[8])?;
                ensure!(!r[7].is_empty(), "Empty description term");
                descriptions.push(Description {
                    id: description,
                    concept: resolve(id(r[4])?)?,
                    module: resolve(id(r[3])?)?,
                    kind: resolve(id(r[6])?)?,
                    effective_time,
                    active: active(r[2])?,
                    language,
                    term: r[7].to_owned(),
                    dialects: Vec::new(),
                });
                Ok(())
            },
        )?;
    }
    let language_files: Vec<_> = archive
        .file_names()
        .filter(|n| {
            n.contains("/Snapshot/")
                && n.ends_with(".txt")
                && n.rsplit('/')
                    .next()
                    .is_some_and(|f| f.starts_with("der2_cRefset_Language"))
        })
        .map(str::to_owned)
        .collect();
    let mut seen = HashSet::new();
    for file in language_files {
        rows(
            archive,
            &file,
            &[
                "id",
                "effectiveTime",
                "active",
                "moduleId",
                "refsetId",
                "referencedComponentId",
                "acceptabilityId",
            ],
            |r| {
                ensure!(seen.insert(r[0].to_owned()), "Duplicate language member ID");
                ensure!(
                    date(r[1])? <= edition_date,
                    "Language member is newer than edition"
                );
                resolve(id(r[3])?)?;
                let refset = resolve(id(r[4])?)?;
                let acceptability = resolve(id(r[6])?)?;
                let position = *positions
                    .get(&id(r[5])?)
                    .context("Language member references a missing description")?;
                if active(r[2])? {
                    descriptions[position]
                        .dialects
                        .push((refset, acceptability));
                }
                Ok(())
            },
        )?;
    }
    Ok(descriptions)
}

pub(super) fn build(
    archive: &mut ZipArchive<BufReader<File>>,
    lookup: &HashMap<u64, u32>,
    edition_date: u32,
) -> Result<DescriptionIndex> {
    DescriptionIndex::build(lookup.len(), read(archive, lookup, edition_date)?)
}
