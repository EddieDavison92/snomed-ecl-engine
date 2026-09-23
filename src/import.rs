use crate::store::{
    sha256, Adjacency, Attribute, Attributes, ConcreteValue, DisplayStore, Manifest, NumericStore,
    FORMAT,
};
use anyhow::{bail, ensure, Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use zip::ZipArchive;
mod descriptions;
mod identifiers;
mod member_schema;
mod members;
mod membership;
mod supplement;
pub use supplement::add_refsets_snapshot;

const ISA: u64 = 116680003;
const INFERRED: u64 = 900000000000011006;
const SYNONYM: u64 = 900000000000013009;
const FSN: u64 = 900000000000003001;
const PREFERRED: u64 = 900000000000548007;
pub const UK_DISPLAY_REFSETS: &[u64] =
    &[999001261000000100, 999000691000001104, 900000000000508004];

pub struct ImportOptions {
    pub edition: String,
    pub expected_sha256: String,
    pub display_refsets: Vec<u64>,
}

/// Imports one self-contained Snapshot package. Full, Delta and package merging are not supported.
pub fn import_snapshot(
    archive_path: &Path,
    destination: &Path,
    options: &ImportOptions,
) -> Result<Manifest> {
    import_snapshot_with_progress(archive_path, destination, options, |_| {})
}

/// Reports stage starts to the caller without writing to stdout or stderr.
/// Progress is informational; successful completion is the returned manifest.
pub fn import_snapshot_with_progress(
    archive_path: &Path,
    destination: &Path,
    options: &ImportOptions,
    mut progress: impl FnMut(&'static str),
) -> Result<Manifest> {
    ensure!(
        !destination.exists(),
        "Destination already exists; choose a new directory"
    );
    ensure!(
        !options.display_refsets.is_empty() && options.display_refsets.len() < 100,
        "Choose 1 to 99 ordered display refsets"
    );
    ensure!(
        options.expected_sha256.len() == 64
            && options
                .expected_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit()),
        "Expected archive SHA-256 is required"
    );
    progress("Verifying archive checksum");
    let archive_hash = sha256(archive_path)?;
    ensure!(
        archive_hash.eq_ignore_ascii_case(&options.expected_sha256),
        "RF2 archive checksum mismatch"
    );
    let mut archive = ZipArchive::new(BufReader::new(File::open(archive_path)?))?;
    let concept_file = snapshot_file(&archive, "sct2_Concept_")?;
    let relationship_file = snapshot_file(&archive, "sct2_Relationship_")?;
    let concrete_file = snapshot_file(&archive, "sct2_RelationshipConcreteValues_")?;
    let description_file = snapshot_file(&archive, "sct2_Description_")?;
    let language_file = snapshot_file(&archive, "der2_cRefset_Language")?;
    let dependencies_file = snapshot_file(&archive, "der2_ssRefset_ModuleDependency")?;
    let metadata_files: Vec<_> = archive
        .file_names()
        .filter(|n| n.ends_with("/release_package_information.json"))
        .map(str::to_owned)
        .collect();
    ensure!(
        metadata_files.len() == 1,
        "One package metadata file is required"
    );
    let mut metadata_text = String::new();
    archive
        .by_name(&metadata_files[0])?
        .take(1024 * 1024)
        .read_to_string(&mut metadata_text)?;
    let package: serde_json::Value = serde_json::from_str(&metadata_text)?;
    let edition_parts: Vec<_> = options.edition.split('/').collect();
    ensure!(
        edition_parts.len() == 7
            && edition_parts[..4] == ["http:", "", "snomed.info", "sct"]
            && edition_parts[5] == "version",
        "Expected a versioned SNOMED edition URI"
    );
    let edition_module = id(edition_parts[4])?;
    let edition_date = date(edition_parts[6])?;
    ensure!(
        package["effectiveTime"].as_str() == Some(edition_parts[6]),
        "Package date differs from edition URI"
    );

    progress("Reading concepts and module dependencies");
    let mut concepts = Vec::new();
    rows(
        &mut archive,
        &concept_file,
        &[
            "id",
            "effectiveTime",
            "active",
            "moduleId",
            "definitionStatusId",
        ],
        |r| {
            let effective = date(r[1])?;
            ensure!(effective <= edition_date, "Concept is newer than edition");
            let definition = match id(r[4])? {
                900000000000074008 => 0,
                900000000000073002 => 2,
                _ => bail!("Unknown definition status"),
            };
            concepts.push((
                id(r[0])?,
                id(r[3])?,
                effective,
                active(r[2])? as u8 | definition,
            ));
            Ok(())
        },
    )?;
    concepts.sort_unstable_by_key(|r| r.0);
    ensure!(
        !concepts.is_empty() && concepts.len() < u32::MAX as usize,
        "Invalid concept population"
    );
    ensure!(
        concepts.windows(2).all(|w| w[0].0 < w[1].0),
        "Duplicate concept IDs in Snapshot"
    );
    let lookup: HashMap<_, _> = concepts
        .iter()
        .enumerate()
        .map(|(i, r)| (r.0, i as u32))
        .collect();
    let resolve = |sctid: u64| -> Result<u32> {
        lookup
            .get(&sctid)
            .copied()
            .context("Referenced concept is missing from package")
    };
    let mut store = NumericStore::default();
    for (sctid, module, effective, flags) in concepts {
        store.ids.push(sctid);
        store.modules.push(resolve(module)?);
        store.effective_times.push(effective);
        store.flags.push(flags);
    }
    let mut dependencies = Vec::new();
    resolve(edition_module)?;
    rows(
        &mut archive,
        &dependencies_file,
        &[
            "id",
            "effectiveTime",
            "active",
            "moduleId",
            "refsetId",
            "referencedComponentId",
            "sourceEffectiveTime",
            "targetEffectiveTime",
        ],
        |r| {
            if !active(r[2])? {
                return Ok(());
            }
            let source = id(r[3])?;
            let target = id(r[5])?;
            resolve(source)?;
            resolve(target)?;
            ensure!(
                date(r[6])? <= edition_date && date(r[7])? <= edition_date,
                "Module dependency is newer than edition"
            );
            dependencies.push(serde_json::json!({"moduleId": source.to_string(), "referencedComponentId": target.to_string(), "sourceEffectiveTime": r[6], "targetEffectiveTime": r[7]}));
            Ok(())
        },
    )?;
    ensure!(
        dependencies
            .iter()
            .any(|d| d["moduleId"].as_str() == Some(edition_parts[4])
                && d["sourceEffectiveTime"] == edition_parts[6]),
        "Edition composition dependency is absent"
    );

    progress("Reading inferred relationships");
    let mut parents = Vec::new();
    let mut attributes = Vec::new();
    let mut seen_relationships = HashSet::new();
    rows(
        &mut archive,
        &relationship_file,
        &[
            "id",
            "effectiveTime",
            "active",
            "moduleId",
            "sourceId",
            "destinationId",
            "relationshipGroup",
            "typeId",
            "characteristicTypeId",
            "modifierId",
        ],
        |r| {
            if !active(r[2])? {
                return Ok(());
            }
            ensure!(
                seen_relationships.insert(id(r[0])?),
                "Duplicate active relationship ID"
            );
            ensure!(
                date(r[1])? <= edition_date,
                "Relationship is newer than edition"
            );
            resolve(id(r[3])?)?;
            if id(r[8])? != INFERRED {
                return Ok(());
            }
            ensure!(
                id(r[9])? == 900000000000451002,
                "Unsupported relationship modifier"
            );
            let source = resolve(id(r[4])?)?;
            let target = resolve(id(r[5])?)?;
            let kind_id = id(r[7])?;
            let kind = resolve(kind_id)?;
            let group: u32 = r[6].parse().context("Invalid relationship group")?;
            if kind_id == ISA {
                ensure!(group == 0, "Grouped is-a relationship");
                parents.push((source, target));
            } else {
                attributes.push((
                    source,
                    Attribute {
                        group,
                        kind,
                        value: target,
                    },
                ));
            }
            Ok(())
        },
    )?;
    let children = parents
        .iter()
        .map(|&(child, parent)| (parent, child))
        .collect();
    let n = store.ids.len();
    store.parents = Adjacency::build(n, parents)?;
    store.children = Adjacency::build(n, children)?;
    store.attributes = Attributes::build(n, attributes)?;
    let mut concrete = Vec::new();
    let mut values = HashMap::new();
    progress("Reading concrete values");
    rows(
        &mut archive,
        &concrete_file,
        &[
            "id",
            "effectiveTime",
            "active",
            "moduleId",
            "sourceId",
            "value",
            "relationshipGroup",
            "typeId",
            "characteristicTypeId",
            "modifierId",
        ],
        |r| {
            if !active(r[2])? {
                return Ok(());
            }
            ensure!(
                seen_relationships.insert(id(r[0])?),
                "Duplicate active relationship ID"
            );
            ensure!(
                date(r[1])? <= edition_date,
                "Concrete relationship is newer than edition"
            );
            resolve(id(r[3])?)?;
            if id(r[8])? != INFERRED {
                return Ok(());
            }
            ensure!(
                id(r[9])? == 900000000000451002,
                "Unsupported relationship modifier"
            );
            let source = resolve(id(r[4])?)?;
            let kind = resolve(id(r[7])?)?;
            let group = r[6]
                .parse()
                .context("Invalid concrete relationship group")?;
            let next = u32::try_from(store.concrete_values.len())?;
            let value = match values.get(r[5]) {
                Some(&value) => value,
                None => {
                    store.concrete_values.push(ConcreteValue::parse(r[5])?);
                    values.insert(r[5].to_owned(), next);
                    next
                }
            };
            concrete.push((source, Attribute { group, kind, value }));
            Ok(())
        },
    )?;
    store.concrete = Attributes::build(n, concrete)?;
    drop(seen_relationships);
    drop(values);
    store.validate()?;

    progress("Indexing concept reference set membership");
    let (membership, non_concept_rows, refset_files) =
        membership::read(&mut archive, &lookup, &store, edition_date)?;
    store.membership = Some(membership);

    progress("Selecting displays in a separate pass");
    let mut preferred: HashMap<u64, u16> = HashMap::new();
    rows(
        &mut archive,
        &language_file,
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
            if !active(r[2])? || id(r[6])? != PREFERRED {
                return Ok(());
            }
            ensure!(
                date(r[1])? <= edition_date,
                "Language member is newer than edition"
            );
            resolve(id(r[3])?)?;
            let refset_id = id(r[4])?;
            if let Some(rank) = options
                .display_refsets
                .iter()
                .position(|&refset| refset_id == refset)
            {
                let rank = rank as u16;
                preferred
                    .entry(id(r[5])?)
                    .and_modify(|old| *old = (*old).min(rank))
                    .or_insert(rank);
            }
            Ok(())
        },
    )?;
    let mut labels = vec![None; n];
    let mut best = vec![(u16::MAX, u64::MAX); n];
    rows(
        &mut archive,
        &description_file,
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
            if !active(r[2])? || r[5] != "en" {
                return Ok(());
            }
            ensure!(
                date(r[1])? <= edition_date,
                "Description is newer than edition"
            );
            let concept = resolve(id(r[4])?)? as usize;
            let description = id(r[0])?;
            let kind = id(r[6])?;
            let rank = if kind == SYNONYM {
                preferred
                    .get(&description)
                    .copied()
                    .unwrap_or(options.display_refsets.len() as u16 + 1)
            } else if kind == FSN {
                options.display_refsets.len() as u16
            } else {
                return Ok(());
            };
            if (rank, description) < best[concept] {
                ensure!(!r[7].is_empty(), "Empty display term");
                best[concept] = (rank, description);
                labels[concept] = Some(r[7].to_owned());
            }
            Ok(())
        },
    )?;
    drop(preferred);
    drop(best);
    progress("Indexing descriptions and language memberships");
    let descriptions = descriptions::build(&mut archive, &lookup, edition_date)?;

    let parent = destination.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let staging = parent.join(format!(".store-building-{}-{nonce}", std::process::id()));
    fs::create_dir(&staging)?;
    progress("Writing immutable store");
    let core_path = staging.join("core.bin");
    let display_path = staging.join("display.bin");
    store.write(&core_path)?;
    DisplayStore::write(&display_path, &labels)?;
    let membership_path = staging.join("membership.bin");
    let membership = store
        .membership
        .as_ref()
        .context("Missing built membership index")?;
    membership.write(&membership_path)?;
    let membership_manifest =
        membership.manifest(&membership_path, non_concept_rows, refset_files)?;
    let description_manifest = descriptions.write(&staging.join("descriptions.bin"))?;
    progress("Indexing description words for search");
    let search_manifest = crate::store::SearchIndex::build(crate::store::search_pairs(&descriptions, n)?)?
        .write(&staging.join("search.bin"))?;
    drop(descriptions);
    progress("Indexing typed reference-set members");
    let member_tables =
        members::build(&mut archive, &lookup, &store, edition_date, &staging, None)?;
    let identifiers = crate::store::IdentifierIndex::build(identifiers::read(
        &mut archive,
        &lookup,
        edition_date,
    )?)?
    .write(&staging)?;
    drop(lookup);
    let manifest = Manifest {
        format: FORMAT,
        edition: options.edition.clone(),
        archive_sha256: archive_hash,
        concept_count: n,
        active_concept_count: store.flags.iter().filter(|&&f| f & 1 != 0).count(),
        hierarchy_edges: store.parents.values.len(),
        attributes: store.attributes.rows.len(),
        concrete_attributes: store.concrete.rows.len(),
        concrete_values: store.concrete_values.len(),
        core_bytes: core_path.metadata()?.len(),
        core_sha256: sha256(&core_path)?,
        display_bytes: display_path.metadata()?.len(),
        display_sha256: sha256(&display_path)?,
        display_refsets: options.display_refsets.clone(),
        displays_selected: labels.iter().flatten().count(),
        module_dependencies: serde_json::Value::Array(dependencies),
        capabilities: vec![
            "concept-metadata".into(),
            "inferred-hierarchy".into(),
            "grouped-relationship-storage".into(),
            "exact-concrete-value-storage".into(),
            "separate-english-display-lookup".into(),
            "concept-refset-membership".into(),
            "complete-description-metadata".into(),
            "typed-concept-refset-members".into(),
            "alternate-identifiers".into(),
        ],
        membership: Some(membership_manifest),
        descriptions: Some(description_manifest),
        member_tables: Some(member_tables),
        identifiers: Some(identifiers),
        search: Some(search_manifest),
        supplements: Vec::new(),
    };
    let mut manifest_file = BufWriter::new(File::create_new(staging.join("manifest.json"))?);
    serde_json::to_writer_pretty(&mut manifest_file, &manifest)?;
    manifest_file.flush()?;
    manifest_file.get_ref().sync_all()?;
    drop(manifest_file);
    ensure!(!destination.exists(), "Destination appeared during import");
    fs::rename(&staging, destination)
        .context("Could not publish store; build directory retained")?;
    Ok(manifest)
}

fn snapshot_file(archive: &ZipArchive<BufReader<File>>, prefix: &str) -> Result<String> {
    let names: Vec<_> = archive
        .file_names()
        .filter(|name| {
            name.contains("/Snapshot/")
                && name.ends_with(".txt")
                && name
                    .rsplit('/')
                    .next()
                    .is_some_and(|file| file.starts_with(prefix))
        })
        .collect();
    ensure!(
        names.len() == 1,
        "Expected one Snapshot file for {prefix}; merged packages are not supported"
    );
    Ok(names[0].to_owned())
}

fn rows(
    archive: &mut ZipArchive<BufReader<File>>,
    name: &str,
    expected: &[&str],
    mut visit: impl FnMut(&[&str]) -> Result<()>,
) -> Result<()> {
    let mut reader = BufReader::new(archive.by_name(name)?);
    let mut line = String::new();
    ensure!(reader.read_line(&mut line)? > 0, "Missing RF2 header");
    let header: Vec<_> = line
        .trim_start_matches('\u{feff}')
        .trim_end_matches(['\r', '\n'])
        .split('\t')
        .collect();
    ensure!(header == expected, "Unexpected RF2 header for {name}");
    let mut number = 1;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        number += 1;
        ensure!(line.len() <= 16 * 1024 * 1024, "RF2 line is too large");
        let columns: Vec<_> = line.trim_end_matches(['\r', '\n']).split('\t').collect();
        ensure!(
            columns.len() == expected.len(),
            "RF2 column count mismatch at {name}:{number}"
        );
        visit(&columns).with_context(|| format!("Invalid RF2 record at {name}:{number}"))?;
    }
    Ok(())
}

fn id(value: &str) -> Result<u64> {
    ensure!(
        !value.is_empty()
            && value.len() <= 18
            && value.bytes().all(|b| b.is_ascii_digit())
            && !value.starts_with('0'),
        "Invalid numeric identifier"
    );
    value.parse().context("Invalid numeric identifier")
}

fn active(value: &str) -> Result<bool> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => bail!("Invalid active flag"),
    }
}

fn date(value: &str) -> Result<u32> {
    ensure!(
        value.len() == 8 && value.bytes().all(|b| b.is_ascii_digit()),
        "Expected published RF2 date"
    );
    let number: u32 = value.parse()?;
    let year = number / 10000;
    let month = number / 100 % 100;
    let day = number % 100;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || year.is_multiple_of(4) && !year.is_multiple_of(100) => 29,
        2 => 28,
        _ => bail!("Invalid RF2 month"),
    };
    ensure!(year >= 1900 && day > 0 && day <= days, "Invalid RF2 date");
    Ok(number)
}

/// What an RF2 archive declares about itself, without importing it.
#[derive(Debug, serde::Serialize)]
pub struct ArchiveSummary {
    pub sha256: String,
    pub bytes: u64,
    pub effective_time: String,
    /// Edition URIs the archive's own module dependencies support, the roots
    /// first. A module that another module depends on is not the edition.
    pub edition_uris: Vec<String>,
    /// How many of those are roots. Exactly one means the edition is unambiguous.
    pub root_editions: usize,
    /// Snapshot files the importer requires, and whether each was found.
    pub required_files: Vec<(String, Option<String>)>,
    pub importable: bool,
}

/// Reads an archive's declared release metadata so the caller can check it
/// against the distributor's published values before importing. This reports
/// what the file claims; it establishes nothing about the file's origin.
pub fn inspect_archive(archive_path: &Path) -> Result<ArchiveSummary> {
    let bytes = std::fs::metadata(archive_path)?.len();
    let sha256 = crate::store::sha256(archive_path)?;
    let mut archive = ZipArchive::new(BufReader::new(File::open(archive_path)?))?;
    let required = [
        ("concepts", "sct2_Concept_"),
        ("relationships", "sct2_Relationship_"),
        ("concrete values", "sct2_RelationshipConcreteValues_"),
        ("descriptions", "sct2_Description_"),
        ("language refset", "der2_cRefset_Language"),
        ("module dependencies", "der2_ssRefset_ModuleDependency"),
    ];
    let required_files: Vec<_> = required
        .iter()
        .map(|(label, prefix)| ((*label).to_owned(), snapshot_file(&archive, prefix).ok()))
        .collect();
    let importable = required_files.iter().all(|(_, found)| found.is_some());

    let metadata_files: Vec<_> = archive
        .file_names()
        .filter(|n| n.ends_with("/release_package_information.json"))
        .map(str::to_owned)
        .collect();
    ensure!(
        metadata_files.len() == 1,
        "One package metadata file is required; this archive has {}",
        metadata_files.len()
    );
    let mut metadata_text = String::new();
    archive
        .by_name(&metadata_files[0])?
        .take(1024 * 1024)
        .read_to_string(&mut metadata_text)?;
    let package: serde_json::Value = serde_json::from_str(&metadata_text)?;
    let effective_time = package["effectiveTime"]
        .as_str()
        .context("Package metadata has no effectiveTime")?
        .to_owned();

    // The importer requires a module whose own dependency row carries the
    // package date, so those modules are the edition URIs it would accept.
    let mut modules = Vec::new();
    let mut depended_on = Vec::new();
    if let Some(Some(name)) = required_files
        .iter()
        .find(|(label, _)| label == "module dependencies")
        .map(|(_, found)| found.clone())
    {
        rows(
            &mut archive,
            &name,
            &[
                "id",
                "effectiveTime",
                "active",
                "moduleId",
                "refsetId",
                "referencedComponentId",
                "sourceEffectiveTime",
                "targetEffectiveTime",
            ],
            |r| {
                if !active(r[2])? {
                    return Ok(());
                }
                if r[6] == effective_time && !modules.contains(&r[3].to_owned()) {
                    modules.push(r[3].to_owned());
                }
                if !depended_on.contains(&r[5].to_owned()) {
                    depended_on.push(r[5].to_owned());
                }
                Ok(())
            },
        )?;
    }
    modules.sort();
    // The edition module is the root of the package's dependency graph: every
    // other module is depended on by something inside the package.
    let (roots, rest): (Vec<_>, Vec<_>) = modules
        .into_iter()
        .partition(|module| !depended_on.contains(module));
    let root_editions = roots.len();
    Ok(ArchiveSummary {
        edition_uris: roots
            .into_iter()
            .chain(rest)
            .map(|module| format!("http://snomed.info/sct/{module}/version/{effective_time}"))
            .collect(),
        root_editions,
        sha256,
        bytes,
        effective_time,
        required_files,
        importable,
    })
}
