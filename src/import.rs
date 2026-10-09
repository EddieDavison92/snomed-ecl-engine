use crate::store::{
    sha256, Adjacency, Attribute, Attributes, ConcreteValue, DisplayStore, Manifest, NumericStore,
    Source, FORMAT,
};
use anyhow::{bail, ensure, Context, Result};
use std::collections::{BTreeSet, HashMap, HashSet};
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
pub const INTERNATIONAL_DISPLAY_REFSETS: &[u64] = &[900000000000509007, 900000000000508004];

/// Ordered language refsets for display labels. International uses US English,
/// then GB English; other editions use the UK defaults. FSNs are the fallback.
pub fn default_display_refsets(edition: &str) -> &'static [u64] {
    if edition
        .strip_prefix("http://snomed.info/sct/")
        .and_then(|rest| rest.split_once("/version/"))
        .is_some_and(|(module, _)| module == "900000000000207008")
    {
        INTERNATIONAL_DISPLAY_REFSETS
    } else {
        UK_DISPLAY_REFSETS
    }
}

pub struct ImportOptions {
    pub edition: String,
    pub expected_sha256: String,
    pub display_refsets: Vec<u64>,
    pub source: Option<Source>,
}

impl ImportOptions {
    /// Uses the edition's display refsets, with no recorded distributor source.
    pub fn new(edition: impl Into<String>, expected_sha256: impl Into<String>) -> Self {
        let edition = edition.into();
        let display_refsets = default_display_refsets(&edition).to_vec();
        Self {
            edition,
            expected_sha256: expected_sha256.into(),
            display_refsets,
            source: None,
        }
    }

    pub fn with_display_refsets(mut self, display_refsets: Vec<u64>) -> Self {
        self.display_refsets = display_refsets;
        self
    }

    pub fn with_source(mut self, source: Source) -> Self {
        self.source = Some(source);
        self
    }
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
/// How many stages `import_snapshot_with_progress` reports.
pub const IMPORT_STAGES: usize = 11;

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
    let mut dependencies = Vec::new();
    let mut missing_modules: BTreeSet<_> = concepts
        .iter()
        .map(|r| r.1)
        .filter(|module| !lookup.contains_key(module))
        .collect();
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
            for module in [source, target] {
                if !lookup.contains_key(&module) {
                    missing_modules.insert(module);
                }
            }
            ensure!(
                date(r[6])? <= edition_date && date(r[7])? <= edition_date,
                "Module dependency is newer than edition"
            );
            dependencies.push(serde_json::json!({"moduleId": source.to_string(), "referencedComponentId": target.to_string(), "sourceEffectiveTime": r[6], "targetEffectiveTime": r[7]}));
            Ok(())
        },
    )?;
    ensure!(
        missing_modules.is_empty(),
        "Missing dependency module concepts: {}. The package is not self-contained; \
         an extension-only package such as the UK Drug Extension needs the editions it depends on",
        missing_modules
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    resolve(edition_module)?;
    ensure!(
        dependencies
            .iter()
            .any(|d| d["moduleId"].as_str() == Some(edition_parts[4])
                && d["sourceEffectiveTime"] == edition_parts[6]),
        "Edition composition dependency is absent"
    );
    let mut store = NumericStore::default();
    for (sctid, module, effective, flags) in concepts {
        store.ids.push(sctid);
        store.modules.push(resolve(module)?);
        store.effective_times.push(effective);
        store.flags.push(flags);
    }

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
    let search_manifest =
        crate::store::SearchIndex::build(crate::store::search_pairs(&descriptions, n)?)?
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
        source: options.source.clone(),
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
        history: None,
        supplements: Vec::new(),
        header_repairs: header_repairs(&mut archive)?,
    };
    let mut manifest_file = BufWriter::new(File::create_new(staging.join("manifest.json"))?);
    serde_json::to_writer_pretty(&mut manifest_file, &manifest)?;
    manifest_file.flush()?;
    manifest_file.get_ref().sync_all()?;
    drop(manifest_file);
    progress("Indexing historical associations");
    let mut manifest = manifest;
    manifest.history = crate::store::add_history(&staging)?;
    ensure!(!destination.exists(), "Destination appeared during import");
    fs::rename(&staging, destination)
        .context("Could not publish store; build directory retained")?;
    Ok(manifest)
}

fn snapshot_file(archive: &ZipArchive<BufReader<File>>, prefix: &str) -> Result<String> {
    let names = snapshot_files(archive, prefix);
    ensure!(
        names.len() == 1,
        "Expected one Snapshot file for {prefix}; merged packages are not supported"
    );
    Ok(names.into_iter().next().unwrap())
}

fn snapshot_files(archive: &ZipArchive<BufReader<File>>, prefix: &str) -> Vec<String> {
    let mut names: Vec<_> = archive
        .file_names()
        .filter(|name| {
            name.contains("/Snapshot/")
                && name.ends_with(".txt")
                && name
                    .rsplit('/')
                    .next()
                    .is_some_and(|file| file.starts_with(prefix))
        })
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

/// The six columns RF2 fixes at the start of every refset file.
const REFSET_COLUMNS: [&str; 6] = [
    "id",
    "effectiveTime",
    "active",
    "moduleId",
    "refsetId",
    "referencedComponentId",
];

/// Other names a distributor has used for a fixed column. UK Monolith 43.0.0
/// names a map refset's referenced component `mapSource`.
const REFSET_ALIASES: [(&str, &str); 1] = [("referencedComponentId", "mapSource")];

/// A refset file's column names, with the first six set to the RF2 names. RF2 fixes those columns by position, so a header that
/// differs only by case or a known alias is read by position, and the returned
/// note says what was read as what. Any other header is rejected.
struct RefsetHeader {
    columns: Vec<String>,
    repair: Option<String>,
}

fn refset_header(archive: &mut ZipArchive<BufReader<File>>, name: &str) -> Result<RefsetHeader> {
    let mut line = String::new();
    BufReader::new(archive.by_name(name)?).read_line(&mut line)?;
    refset_header_from(name, &line)
}

/// Snapshot refset files, by the name test every refset reader uses.
fn refset_files(archive: &ZipArchive<BufReader<File>>) -> Vec<String> {
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
    names
}

/// The repair note for every refset file in the package with misnamed fixed
/// columns, so each is recorded once whichever readers open it.
fn header_repairs(archive: &mut ZipArchive<BufReader<File>>) -> Result<Vec<String>> {
    let mut repairs = Vec::new();
    for name in refset_files(archive) {
        repairs.extend(refset_header(archive, &name)?.repair);
    }
    Ok(repairs)
}

fn refset_header_from(name: &str, line: &str) -> Result<RefsetHeader> {
    let written: Vec<String> = line
        .trim_start_matches('\u{feff}')
        .trim_end_matches(['\r', '\n'])
        .split('\t')
        .map(str::to_owned)
        .collect();
    ensure!(
        written.len() >= REFSET_COLUMNS.len(),
        "Unexpected refset Snapshot header for {name}"
    );
    let mut columns = written.clone();
    let mut renamed = Vec::new();
    for (column, standard) in columns.iter_mut().zip(REFSET_COLUMNS) {
        if column == standard {
            continue;
        }
        let known = column.eq_ignore_ascii_case(standard)
            || REFSET_ALIASES.contains(&(standard, column.as_str()));
        ensure!(known, "Unexpected refset Snapshot header for {name}");
        renamed.push(format!("{column} as {standard}"));
        *column = standard.to_owned();
    }
    let file = name.rsplit('/').next().unwrap_or(name);
    let repair = (!renamed.is_empty()).then(|| format!("{file}: read {}", renamed.join(", ")));
    Ok(RefsetHeader { columns, repair })
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
    // A refset file's fixed columns may be misnamed in the ways refset_header
    // allows; every other column, and every other file, must match exactly.
    let matches = if expected.starts_with(&REFSET_COLUMNS) {
        refset_header_from(name, &line).is_ok_and(|header| header.columns == expected)
    } else {
        let header: Vec<_> = line
            .trim_start_matches('\u{feff}')
            .trim_end_matches(['\r', '\n'])
            .split('\t')
            .collect();
        header == expected
    };
    ensure!(matches, "Unexpected RF2 header for {name}");
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
    /// first. A root may be an edition or a dependent module that nothing else
    /// in this package depends on.
    pub edition_uris: Vec<String>,
    /// Number of roots. A single root is the default when no edition module is known.
    pub root_editions: usize,
    /// Snapshot files the importer requires, and whether each was found.
    pub required_files: Vec<(String, Option<String>)>,
    /// File kinds with multiple matches, and every matching archive path.
    pub duplicate_files: Vec<(String, Vec<String>)>,
    pub importable: bool,
}

impl ArchiveSummary {
    /// Chooses the expected edition module among all candidates, or the single
    /// root when no module is known. An explicit URI can bypass this choice.
    pub fn choose_edition(&self, expected_module: Option<u64>) -> Result<&str> {
        if let Some(module) = expected_module {
            let expected = format!(
                "http://snomed.info/sct/{module}/version/{}",
                self.effective_time
            );
            if let Some(uri) = self.edition_uris.iter().find(|uri| **uri == expected) {
                return Ok(uri);
            }
            bail!(
                "The archive has no edition URI for expected module {module}. Candidates: {}. Choose one with --edition URI",
                self.edition_uris.join(", ")
            );
        }
        if self.root_editions == 1 {
            return Ok(&self.edition_uris[0]);
        }
        bail!(
            "The archive does not name one edition. Candidates: {}. Choose one with --edition URI",
            self.edition_uris.join(", ")
        )
    }
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
    let mut duplicate_files = Vec::new();
    let required_files: Vec<_> = required
        .iter()
        .map(|(label, prefix)| {
            let names = snapshot_files(&archive, prefix);
            let found = if names.len() == 1 {
                names.into_iter().next()
            } else {
                if names.len() > 1 {
                    duplicate_files.push(((*label).to_owned(), names));
                }
                None
            };
            ((*label).to_owned(), found)
        })
        .collect();

    let mut metadata_files: Vec<_> = archive
        .file_names()
        .filter(|n| n.ends_with("/release_package_information.json"))
        .map(str::to_owned)
        .collect();
    ensure!(
        !metadata_files.is_empty(),
        "One package metadata file is required; this archive has {}",
        metadata_files.len()
    );
    let effective_time = if metadata_files.len() == 1 {
        let mut metadata_text = String::new();
        archive
            .by_name(&metadata_files[0])?
            .take(1024 * 1024)
            .read_to_string(&mut metadata_text)?;
        let package: serde_json::Value = serde_json::from_str(&metadata_text)?;
        package["effectiveTime"]
            .as_str()
            .context("Package metadata has no effectiveTime")?
            .to_owned()
    } else {
        metadata_files.sort();
        duplicate_files.push(("package metadata".into(), metadata_files));
        String::new()
    };
    let importable =
        required_files.iter().all(|(_, found)| found.is_some()) && duplicate_files.is_empty();

    // The importer requires a module whose own dependency row carries the
    // package date, so those modules are the edition URIs it would accept.
    let mut modules = Vec::new();
    let mut depended_on = Vec::new();
    if let Some(Some(name)) = required_files
        .iter()
        .find(|(label, _)| label == "module dependencies")
        .map(|(_, found)| found.clone())
        .filter(|_| !effective_time.is_empty())
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
    // Roots are candidates, not proof of an edition: a map module can also be
    // a root when nothing inside the package depends on it.
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
        duplicate_files,
        importable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refset_headers_read_misnamed_fixed_columns_by_position() {
        let name = "Pkg/Snapshot/Refset/Map/der2_ccRefset_MapSnapshot.txt";
        let standard = "\u{feff}id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tmapTarget\r\n";
        let header = refset_header_from(name, standard).unwrap();
        assert!(header.repair.is_none());
        // UK Monolith 43.0.0 wrote this header for its SNOMED to SNOMED map.
        let uk =
            "id\teffectiveTime\tactive\tmoduleId\trefsetid\tmapSource\tmapTarget\tcorelationId\n";
        let header = refset_header_from(name, uk).unwrap();
        assert_eq!(
            header.columns,
            [
                "id",
                "effectiveTime",
                "active",
                "moduleId",
                "refsetId",
                "referencedComponentId",
                "mapTarget",
                "corelationId"
            ]
        );
        assert_eq!(
            header.repair.as_deref(),
            Some("der2_ccRefset_MapSnapshot.txt: read refsetid as refsetId, mapSource as referencedComponentId")
        );
        // Anything else in the fixed columns is still rejected.
        for bad in [
            "id\teffectiveTime\tactive\tmoduleId\trefsetId\tmapTarget\n",
            "id\teffectiveTime\tactive\tmoduleId\trefsetId\n",
            "uuid\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\n",
            "id\teffectiveTime\tactive\tmoduleId\tmapSource\treferencedComponentId\n",
        ] {
            assert!(refset_header_from(name, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn display_defaults_follow_the_edition_module() {
        let international = "http://snomed.info/sct/900000000000207008/version/20260801";
        assert_eq!(
            default_display_refsets(international),
            INTERNATIONAL_DISPLAY_REFSETS
        );
        for edition in [
            "http://snomed.info/sct/83821000000107/version/20260826",
            "http://snomed.info/sct/1000001/version/20260801",
            "http://snomed.info/sct/9000000000002070080/version/20260801",
        ] {
            assert_eq!(default_display_refsets(edition), UK_DISPLAY_REFSETS);
        }
        assert_eq!(
            ImportOptions::new(international, "a".repeat(64)).display_refsets,
            INTERNATIONAL_DISPLAY_REFSETS
        );
        assert_eq!(
            ImportOptions::new(international, "a".repeat(64))
                .with_display_refsets(vec![1])
                .display_refsets,
            [1]
        );
    }

    #[test]
    fn expected_module_selects_among_all_candidates_and_never_guesses() {
        let international = "http://snomed.info/sct/900000000000207008/version/20260801";
        let map = "http://snomed.info/sct/2000002/version/20260801";
        let mut summary = ArchiveSummary {
            sha256: String::new(),
            bytes: 0,
            effective_time: "20260801".into(),
            edition_uris: vec![map.into(), international.into()],
            root_editions: 2,
            required_files: vec![],
            duplicate_files: vec![],
            importable: true,
        };
        assert_eq!(
            summary.choose_edition(Some(900000000000207008)).unwrap(),
            international
        );
        assert!(summary.choose_edition(None).is_err());
        // Expected modules may also be non-roots.
        summary.root_editions = 1;
        assert_eq!(
            summary.choose_edition(Some(900000000000207008)).unwrap(),
            international
        );
        assert_eq!(summary.choose_edition(None).unwrap(), map);
        let error = summary
            .choose_edition(Some(83821000000107))
            .unwrap_err()
            .to_string();
        for text in ["83821000000107", international, map, "--edition URI"] {
            assert!(error.contains(text), "{error}");
        }
        summary.edition_uris.clear();
        summary.root_editions = 0;
        assert!(summary.choose_edition(None).is_err());
        assert!(summary.choose_edition(Some(900000000000207008)).is_err());
    }
}
