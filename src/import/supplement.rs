use super::*;
use crate::store::{MembershipIndex, RefsetSupplement};
use std::collections::BTreeSet;

/// Adds simple Snapshot refsets and their new definitions to a fresh immutable store.
/// Existing concept definitions and populated refsets cannot be replaced or extended.
/// Other refset payloads remain outside this command's current capabilities.
pub fn add_refsets_snapshot(
    base: &Path,
    archive_path: &Path,
    destination: &Path,
    release_date: &str,
    expected_sha256: &str,
) -> Result<Manifest> {
    ensure!(
        !destination.exists(),
        "Destination already exists; choose a new directory"
    );
    let release_date = date(release_date)?;
    let archive_hash = sha256(archive_path)?;
    ensure!(
        archive_hash.eq_ignore_ascii_case(expected_sha256),
        "RF2 archive checksum mismatch"
    );
    let mut manifest = Manifest::read(base)?;
    ensure!(
        manifest.membership.is_some(),
        "Base membership index is absent; reimport the base RF2 first"
    );
    ensure!(
        !manifest
            .supplements
            .iter()
            .any(|s| s.archive_sha256 == archive_hash),
        "This supplement is already loaded"
    );
    let original = NumericStore::open(base)?;
    let mut archive = ZipArchive::new(BufReader::new(File::open(archive_path)?))?;
    let mut names: Vec<_> = archive
        .file_names()
        .filter(|n| n.contains("/Snapshot/") && n.ends_with(".txt"))
        .map(str::to_owned)
        .collect();
    names.sort();
    ensure!(!names.is_empty(), "No RF2 Snapshot files found");
    let mut additions = std::collections::BTreeMap::new();
    for name in matching(&names, "sct2_Concept_") {
        rows(
            &mut archive,
            name,
            &[
                "id",
                "effectiveTime",
                "active",
                "moduleId",
                "definitionStatusId",
            ],
            |r| {
                let code = id(r[0])?;
                ensure!(original.ordinal(code).is_none(), "Supplement would replace an existing concept; build from the original base instead");
                let effective = date(r[1])?;
                ensure!(
                    effective <= release_date,
                    "Concept is newer than supplement release"
                );
                let definition = match id(r[4])? {
                    900000000000074008 => 0,
                    900000000000073002 => 2,
                    _ => bail!("Unknown definition status"),
                };
                ensure!(
                    additions
                        .insert(
                            code,
                            (id(r[3])?, effective, active(r[2])? as u8 | definition)
                        )
                        .is_none(),
                    "Duplicate supplement concept"
                );
                Ok(())
            },
        )?;
    }
    let mut ids = original.ids.clone();
    ids.extend(additions.keys());
    ids.sort_unstable();
    ensure!(
        ids.len() < u32::MAX as usize,
        "Concept index exceeds u32 capacity"
    );
    let lookup: HashMap<_, _> = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i as u32))
        .collect();
    let resolve = |code| {
        lookup
            .get(&code)
            .copied()
            .context("Supplement references a missing concept")
    };
    let mapping: Vec<_> = original.ids.iter().map(|id| lookup[id]).collect();
    let mut store = NumericStore {
        ids,
        ..NumericStore::default()
    };
    for &code in &store.ids {
        let (module, effective, flags) = if let Some(&row) = additions.get(&code) {
            row
        } else {
            let i = original.ordinal(code).unwrap() as usize;
            (
                original.ids[original.modules[i] as usize],
                original.effective_times[i],
                original.flags[i],
            )
        };
        store.modules.push(resolve(module)?);
        store.effective_times.push(effective);
        store.flags.push(flags);
    }
    let n = store.ids.len();
    let mut parents = Vec::with_capacity(original.parents.values.len());
    let mut attributes = Vec::with_capacity(original.attributes.rows.len());
    let mut concrete = Vec::with_capacity(original.concrete.rows.len());
    for (old, &new) in mapping.iter().enumerate() {
        parents.extend(
            original
                .parents
                .get(old as u32)
                .iter()
                .map(|&p| (new, mapping[p as usize])),
        );
        attributes.extend(original.attributes.get(old as u32).iter().map(|r| {
            (
                new,
                Attribute {
                    group: r.group,
                    kind: mapping[r.kind as usize],
                    value: mapping[r.value as usize],
                },
            )
        }));
        concrete.extend(original.concrete.get(old as u32).iter().map(|r| {
            (
                new,
                Attribute {
                    group: r.group,
                    kind: mapping[r.kind as usize],
                    value: r.value,
                },
            )
        }));
    }
    let mut seen_relationships = HashSet::new();
    for name in matching(&names, "sct2_Relationship_") {
        rows(
            &mut archive,
            name,
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
                ensure!(
                    date(r[1])? <= release_date,
                    "Relationship is newer than supplement release"
                );
                if !active(r[2])? {
                    return Ok(());
                }
                ensure!(
                    seen_relationships.insert(id(r[0])?),
                    "Duplicate supplement relationship"
                );
                ensure!(
                    additions.contains_key(&id(r[4])?),
                    "Supplement would change existing concept relationships"
                );
                resolve(id(r[3])?)?;
                ensure!(
                    id(r[8])? == INFERRED && id(r[9])? == 900000000000451002,
                    "Expected published inferred existential relationships"
                );
                ensure!(
                    id(r[7])? == ISA && r[6] == "0",
                    "Refset definitions currently support ungrouped is-a relationships only"
                );
                parents.push((resolve(id(r[4])?)?, resolve(id(r[5])?)?));
                Ok(())
            },
        )?;
    }
    // Reject unsupported concrete definition payloads rather than silently discarding them.
    for name in matching(&names, "sct2_RelationshipConcreteValues_") {
        rows(
            &mut archive,
            name,
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
                ensure!(
                    !active(r[2])?,
                    "Concrete extension definitions are not supported by add-refsets"
                );
                Ok(())
            },
        )?;
    }
    store.children = Adjacency::build(n, parents.iter().map(|&(c, p)| (p, c)).collect())?;
    store.parents = Adjacency::build(n, parents)?;
    store.attributes = Attributes::build(n, attributes)?;
    store.concrete = Attributes::build(n, concrete)?;

    let existing = original.membership.as_ref().unwrap();
    let mut pairs = Vec::with_capacity(existing.members.len());
    for (position, &refset) in existing.refsets.iter().enumerate() {
        pairs.extend(
            existing
                .get(position)
                .iter()
                .map(|&m| (mapping[refset as usize], mapping[m as usize])),
        );
    }
    let mut refsets = BTreeSet::new();
    let mut member_ids = HashSet::new();
    let simple_files = matching(&names, "der2_Refset_Simple");
    ensure!(
        !simple_files.is_empty(),
        "No simple Snapshot refset files found"
    );
    for name in &simple_files {
        rows(
            &mut archive,
            name,
            &[
                "id",
                "effectiveTime",
                "active",
                "moduleId",
                "refsetId",
                "referencedComponentId",
            ],
            |r| {
                ensure!(
                    date(r[1])? <= release_date,
                    "Member is newer than supplement release"
                );
                ensure!(
                    !r[0].is_empty() && member_ids.insert(r[0].to_ascii_lowercase()),
                    "Duplicate or empty Snapshot member identifier"
                );
                resolve(id(r[3])?)?;
                let refset_id = id(r[4])?;
                let refset = resolve(refset_id)?;
                if let Some(old) = original.ordinal(refset_id) {
                    ensure!(
                        existing.refsets.binary_search(&old).is_err(),
                        "Supplement would change a populated refset; use the original base instead"
                    );
                }
                ensure!(
                    !manifest
                        .supplements
                        .iter()
                        .any(|s| s.refset_ids.contains(&refset_id.to_string())),
                    "Supplement refset is already loaded"
                );
                refsets.insert(refset_id);
                let referenced = id(r[5])?;
                ensure!(
                    matches!((referenced / 10) % 100, 0 | 10),
                    "Only concept-based simple refsets are supported by add-refsets"
                );
                let member = resolve(referenced)?;
                if active(r[2])? {
                    pairs.push((refset, member));
                }
                Ok(())
            },
        )?;
    }
    ensure!(!refsets.is_empty(), "Supplement has no simple refset rows");
    let mut dependencies = Vec::new();
    let base_date = date(
        manifest
            .edition
            .rsplit('/')
            .next()
            .context("Invalid base edition")?,
    )?;
    for name in matching(&names, "der2_ssRefset_ModuleDependency") {
        rows(
            &mut archive,
            name,
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
                ensure!(
                    date(r[1])? <= release_date && date(r[6])? <= release_date,
                    "Dependency is newer than supplement release"
                );
                resolve(id(r[3])?)?;
                ensure!(
                    original.ordinal(id(r[5])?).is_some() && date(r[7])? <= base_date,
                    "Dependency is absent or newer than the base edition"
                );
                dependencies.push(serde_json::json!({"moduleId":r[3],"referencedComponentId":r[5],"sourceEffectiveTime":r[6],"targetEffectiveTime":r[7]}));
                Ok(())
            },
        )?;
    }
    store.membership = Some(MembershipIndex::build(n, pairs)?);
    let mut labels = vec![None; n];
    for (label, &new) in DisplayStore::open(base)?
        .into_labels()?
        .into_iter()
        .zip(&mapping)
    {
        labels[new as usize] = label;
    }
    // New definitions use an active English FSN; existing preferred displays are preserved.
    let mut best = HashMap::new();
    for name in matching(&names, "sct2_Description_") {
        rows(
            &mut archive,
            name,
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
                ensure!(
                    date(r[1])? <= release_date,
                    "Description is newer than supplement release"
                );
                if !active(r[2])? {
                    return Ok(());
                }
                let concept = id(r[4])?;
                ensure!(
                    additions.contains_key(&concept),
                    "Supplement would change existing concept descriptions"
                );
                if r[5] == "en" && id(r[6])? == FSN {
                    ensure!(!r[7].is_empty(), "Empty display term");
                    let description = id(r[0])?;
                    let prior = best.entry(concept).or_insert(u64::MAX);
                    if description < *prior {
                        labels[resolve(concept)? as usize] = Some(r[7].to_owned());
                        *prior = description;
                    }
                }
                Ok(())
            },
        )?;
    }
    store.concrete_values = original.concrete_values;
    let descriptions = if let Some(index) = original.descriptions.into_index()? {
        let mut rows = index.into_descriptions(&mapping);
        let extra = super::descriptions::read(&mut archive, &lookup, release_date)?;
        ensure!(
            extra
                .iter()
                .all(|r| additions.contains_key(&store.ids[r.concept as usize])),
            "Supplement would change existing concept descriptions"
        );
        rows.extend(extra);
        Some(crate::store::DescriptionIndex::build(n, rows)?)
    } else {
        None
    };
    store.validate()?;
    let parent = destination.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let staging = parent.join(format!(".store-building-{}-{nonce}", std::process::id()));
    fs::create_dir(&staging)?;
    let extra_identifiers = super::identifiers::read(&mut archive, &lookup, release_date)?;
    if let Some(base_ids) = original.identifiers.get()? {
        let mut rows = base_ids.rows.clone();
        rows.extend(extra_identifiers);
        manifest.identifiers = Some(crate::store::IdentifierIndex::build(rows)?.write(&staging)?);
    } else {
        ensure!(
            extra_identifiers.is_empty(),
            "Base identifier index is absent; reimport the base before adding identifiers"
        );
    }
    if let Some(tables) = &mut manifest.member_tables {
        fs::create_dir(staging.join("members"))?;
        let additional = super::members::build(
            &mut archive,
            &lookup,
            release_date,
            &staging,
            Some(&original.member_tables),
        )?;
        tables.retain(|old| !additional.iter().any(|new| new.refset == old.refset));
        for table in tables.iter() {
            let relative = Path::new("members").join(format!("{}.bin", table.refset));
            let source = base.join(&relative);
            ensure!(
                source.metadata()?.len() == table.bytes && sha256(&source)? == table.sha256,
                "Base member table checksum differs"
            );
            fs::copy(&source, staging.join(&relative))?;
        }
        tables.extend(additional);
        tables.sort_by_key(|t| t.refset);
    }
    let core = staging.join("core.bin");
    let display = staging.join("display.bin");
    let membership = staging.join("membership.bin");
    store.write(&core)?;
    DisplayStore::write(&display, &labels)?;
    manifest.descriptions = descriptions
        .map(|index| index.write(&staging.join("descriptions.bin")))
        .transpose()?;
    let index = store.membership.as_ref().unwrap();
    index.write(&membership)?;
    let previous = manifest.membership.as_ref().unwrap();
    manifest.membership = Some(index.manifest(
        &membership,
        previous.active_non_concept_rows,
        previous.snapshot_files + simple_files.len(),
    )?);
    manifest.supplements.push(RefsetSupplement {
        archive_sha256: archive_hash,
        release_date,
        base_core_sha256: manifest.core_sha256.clone(),
        refset_ids: refsets.iter().map(u64::to_string).collect(),
        added_concepts: additions.len(),
        module_dependencies: serde_json::Value::Array(dependencies),
        exact_module_versions_verified: false,
    });
    manifest.concept_count = n;
    manifest.active_concept_count = store.flags.iter().filter(|&&f| f & 1 != 0).count();
    manifest.hierarchy_edges = store.parents.values.len();
    manifest.core_bytes = core.metadata()?.len();
    manifest.core_sha256 = sha256(&core)?;
    manifest.display_bytes = display.metadata()?.len();
    manifest.display_sha256 = sha256(&display)?;
    manifest.displays_selected = labels.iter().flatten().count();
    let mut file = File::create_new(staging.join("manifest.json"))?;
    serde_json::to_writer_pretty(&mut file, &manifest)?;
    file.sync_all()?;
    drop(file);
    ensure!(!destination.exists(), "Destination appeared during import");
    fs::rename(&staging, destination)
        .context("Could not publish store; build directory retained")?;
    Ok(manifest)
}

fn matching<'a>(names: &'a [String], prefix: &str) -> Vec<&'a str> {
    names
        .iter()
        .filter(|n| n.rsplit('/').next().is_some_and(|f| f.starts_with(prefix)))
        .map(String::as_str)
        .collect()
}
