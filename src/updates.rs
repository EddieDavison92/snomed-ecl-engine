//! Compare indexed archive identities with TRUD metadata, without downloading RF2.
use crate::{download, library, presentation};
use anyhow::{bail, Context, Result};
use download::Release;
use snomed_ecl_engine::store::{Manifest, Source};
use std::path::{Path, PathBuf};

pub struct Indexed {
    pub name: String,
    pub path: PathBuf,
    pub edition: String,
    pub archive_sha256: String,
    pub source: Option<Source>,
}

impl Indexed {
    fn read(path: &Path) -> Result<Self> {
        let manifest = Manifest::read(path)
            .with_context(|| format!("Cannot read index {}", path.display()))?;
        Ok(Self {
            name: path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path: path.to_path_buf(),
            edition: manifest.edition,
            archive_sha256: manifest.archive_sha256,
            source: manifest.source,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    UpToDate,
    Reissued,
    Behind { releases_behind: usize },
    Unknown,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Self::UpToDate => "up_to_date",
            Self::Reissued => "reissued",
            Self::Behind { .. } => "behind",
            Self::Unknown => "unknown",
        }
    }
}

pub struct Assessment<'a> {
    pub status: Status,
    pub indexed: Option<&'a Indexed>,
    pub indexed_release: Option<&'a Release>,
    pub latest: Option<&'a Release>,
}

/// Releases are newest first. Compare against the most recent indexed release,
/// including a source ID whose archive has since been replaced by TRUD.
pub fn assess<'a>(item: u32, releases: &'a [Release], indexed: &'a [Indexed]) -> Assessment<'a> {
    let indexed = indexed.iter().filter(|index| {
        index
            .source
            .as_ref()
            .is_none_or(|source| source.item == item)
    });
    let latest = releases.first();
    let matched = releases.iter().enumerate().find_map(|(position, release)| {
        // An exact archive match takes precedence over a replaced source ID.
        indexed
            .clone()
            .find(|index| {
                index
                    .archive_sha256
                    .eq_ignore_ascii_case(&release.archive_file_sha256)
            })
            .or_else(|| {
                indexed.clone().find(|index| {
                    index
                        .source
                        .as_ref()
                        .is_some_and(|source| position == 0 && source.release_id == release.id)
                })
            })
            .map(|index| (position, release, index))
    });
    let (status, selected, indexed_release) = match (latest, matched) {
        (Some(latest), Some((position, release, index))) => {
            let status = if index
                .archive_sha256
                .eq_ignore_ascii_case(&latest.archive_file_sha256)
            {
                Status::UpToDate
            } else if release.name == latest.name
                || index
                    .source
                    .as_ref()
                    .is_some_and(|source| source.release_id == latest.id)
            {
                Status::Reissued
            } else {
                Status::Behind {
                    releases_behind: position,
                }
            };
            (status, Some(index), Some(release))
        }
        _ => (
            Status::Unknown,
            indexed
                .filter_map(|index| {
                    let edition = library::edition_parts(&index.edition);
                    if item_family(item).is_some_and(|expected| {
                        edition
                            .as_ref()
                            .is_none_or(|(family, _)| expected != family)
                    }) {
                        return None;
                    }
                    Some((index, edition.map(|(_, date)| date)))
                })
                .max_by(|(a, a_date), (b, b_date)| {
                    fn date(index: &Indexed) -> Option<&str> {
                        index
                            .source
                            .as_ref()
                            .map(|source| source.release_date.as_str())
                    }
                    a_date
                        .cmp(b_date)
                        .then_with(|| date(a).cmp(&date(b)))
                        .then_with(|| b.name.cmp(&a.name))
                })
                .map(|(index, _)| index),
            None,
        ),
    };
    Assessment {
        status,
        indexed: selected,
        indexed_release,
        latest,
    }
}

fn item_name(item: u32) -> Option<&'static str> {
    download::ITEMS
        .iter()
        .find(|(_, number, _)| *number == item)
        .map(|(name, _, _)| *name)
}

fn item_family(item: u32) -> Option<&'static str> {
    item_name(item).and_then(|name| name.strip_suffix("-monolith"))
}

/// The first eight digits following an underscore, including timestamp names.
fn archive_date(name: &str) -> Option<&str> {
    name.split('_').skip(1).find_map(|part| {
        let date = part.get(..8)?;
        date.bytes().all(|b| b.is_ascii_digit()).then_some(date)
    })
}

pub fn suggested_name(
    item: u32,
    latest: &Release,
    indexed: Option<&Indexed>,
    names: &[String],
) -> Option<String> {
    let family = item_family(item).map(str::to_owned).or_else(|| {
        indexed.and_then(|index| library::edition_parts(&index.edition).map(|(family, _)| family))
    })?;
    let date = archive_date(&latest.id).or_else(|| archive_date(&latest.archive_file_name))?;
    let base = format!("{family}-{date}");
    Some(if names.contains(&base) {
        format!(
            "{base}-{}",
            latest.archive_file_sha256.get(..8)?.to_ascii_lowercase()
        )
    } else {
        base
    })
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

pub fn suggested_command(
    item: u32,
    assessment: &Assessment<'_>,
    names: &[String],
) -> Option<String> {
    if assessment.status == Status::UpToDate {
        return None;
    }
    let latest = assessment.latest?;
    if let Err(error) = download::check_file_name(&latest.id) {
        eprintln!("Warning: no download command suggested: {error}");
        return None;
    }
    let name = suggested_name(item, latest, assessment.indexed, names);
    let item = item_name(item).map_or_else(|| item.to_string(), str::to_owned);
    let mut command = format!(
        "snomed-ecl-engine download {} --release {}",
        shell_quote(&item),
        shell_quote(&latest.id)
    );
    if let Some(name) = name {
        command.push_str(&format!(" --name {}", shell_quote(&name)));
    }
    Some(command)
}

/// Only these fields cross the output boundary; the download URL stays private.
pub fn render_json(
    item: u32,
    assessment: &Assessment<'_>,
    command: Option<&str>,
    key: &str,
) -> String {
    let clean = |text: &str| download::redact(text, key);
    let indexed = assessment.indexed.map(|index| {
        let mut value = serde_json::json!({
            "name": clean(&index.name), "path": clean(&index.path.to_string_lossy()),
            "archive_sha256": clean(&index.archive_sha256),
        });
        if let Some(source) = &index.source {
            value["release_id"] = clean(&source.release_id).into();
            value["release_name"] = clean(&source.release_name).into();
        } else if let Some(release) = assessment.indexed_release {
            value["release_id"] = clean(&release.id).into();
            value["release_name"] = clean(&release.name).into();
        }
        value
    });
    let latest = assessment.latest.map(|release| serde_json::json!({
        "id": clean(&release.id), "name": clean(&release.name), "release_date": clean(&release.release_date),
        "bytes": release.archive_file_size_bytes, "sha256": clean(&release.archive_file_sha256),
    }));
    let mut value = serde_json::json!({
        "item": item, "item_name": item_name(item).map(clean), "status": assessment.status.name(),
        "indexed": indexed, "latest": latest,
    });
    if let Status::Behind { releases_behind } = assessment.status {
        value["releases_behind"] = releases_behind.into();
    }
    if let Some(command) = command {
        value["command"] = clean(command).into();
    }
    value.to_string()
}

pub fn render_human(
    item: u32,
    assessment: &Assessment<'_>,
    command: Option<&str>,
    key: &str,
) -> String {
    let clean = |text: &str| presentation::clean(&download::redact(text, key));
    let prefix = |hash: &str| clean(&hash.chars().take(8).collect::<String>());
    let mut output = format!("{} ({item})\n", item_name(item).unwrap_or("TRUD item"));
    if let Some(index) = assessment.indexed {
        let release = index
            .source
            .as_ref()
            .map(|source| source.release_name.as_str())
            .or_else(|| {
                assessment
                    .indexed_release
                    .map(|release| release.name.as_str())
            })
            .unwrap_or("release unknown");
        output.push_str(&format!(
            "  Indexed: {} / {} / SHA-256 {}\n",
            clean(&index.name),
            clean(release),
            prefix(&index.archive_sha256)
        ));
    } else {
        output.push_str("  Indexed: none\n");
    }
    if let Some(latest) = assessment.latest {
        output.push_str(&format!(
            "  TRUD newest: {} / {} / SHA-256 {}\n",
            clean(&latest.name),
            clean(&latest.release_date),
            prefix(&latest.archive_file_sha256)
        ));
    } else {
        output.push_str("  TRUD newest: none\n");
    }
    let status = match assessment.status {
        Status::UpToDate => "up to date".into(),
        Status::Reissued => "reissued".into(),
        Status::Behind { releases_behind } => format!("behind by {releases_behind} releases"),
        Status::Unknown => "unknown".into(),
    };
    output.push_str(&format!("  Status: {status}\n"));
    if let Some(command) = command {
        output.push_str(&format!("  {}\n", clean(command)));
    }
    output
}

pub fn exit_code(statuses: &[Status]) -> i32 {
    if statuses
        .iter()
        .any(|status| matches!(status, Status::Behind { .. } | Status::Reissued))
    {
        3
    } else if statuses.contains(&Status::Unknown) {
        4
    } else {
        0
    }
}

struct Options {
    items: Vec<u32>,
    indexes: Vec<PathBuf>,
    exit_code: bool,
    help: bool,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self> {
        let mut options = Self {
            items: Vec::new(),
            indexes: Vec::new(),
            exit_code: false,
            help: false,
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => options.help = true,
                "--exit-code" => options.exit_code = true,
                "--index" => {
                    let path = args
                        .next()
                        .filter(|arg| !arg.starts_with('-'))
                        .context("--index needs a path")?;
                    options.indexes.push(PathBuf::from(path));
                }
                flag if flag.starts_with('-') => bail!("Unknown updates option {flag}"),
                name => {
                    let item = download::item(name)?;
                    if !options.items.contains(&item) {
                        options.items.push(item);
                    }
                }
            }
        }
        Ok(options)
    }

    fn load_indexes<'a>(
        &mut self,
        library_paths: impl Iterator<Item = &'a Path>,
    ) -> Result<Vec<Indexed>> {
        let indexes = if self.indexes.is_empty() {
            library_paths
                .map(Indexed::read)
                .collect::<Result<Vec<_>>>()?
        } else {
            self.indexes
                .iter()
                .map(|path| Indexed::read(path))
                .collect::<Result<_>>()?
        };
        if self.items.is_empty() {
            self.items = default_items(&indexes);
        }
        Ok(indexes)
    }
}

fn default_items(indexed: &[Indexed]) -> Vec<u32> {
    let mut items: Vec<_> = indexed
        .iter()
        .filter_map(|index| index.source.as_ref().map(|source| source.item))
        .collect();
    items.sort_unstable();
    items.dedup();
    if items.is_empty() {
        items.push(1799);
    }
    items
}

pub fn run(args: &[String], human: bool) -> Result<i32> {
    use std::io::Write;
    let mut options = Options::parse(args)?;
    if options.help {
        presentation::help(Some("updates"))?;
        return Ok(0);
    }
    let key = download::key()?;
    let library_entries = library::entries();
    let names: Vec<_> = library_entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    let indexes = options.load_indexes(library_entries.iter().map(|entry| entry.path.as_path()))?;
    let mut statuses = Vec::new();
    let mut out = std::io::stdout().lock();
    for item in options.items {
        let releases = download::releases_for_updates(item, &key)?;
        let assessment = assess(item, &releases, &indexes);
        let command = suggested_command(item, &assessment, &names);
        if human {
            write!(
                out,
                "{}",
                render_human(item, &assessment, command.as_deref(), &key)
            )?;
        } else {
            writeln!(
                out,
                "{}",
                render_json(item, &assessment, command.as_deref(), &key)
            )?;
        }
        statuses.push(assessment.status);
    }
    out.flush()?;
    Ok(if options.exit_code {
        exit_code(&statuses)
    } else {
        0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const KEY: &str = "SENTINEL-KEY-123";
    const RECORDED: &str = include_str!("updates/test-releases.json");

    fn releases() -> Vec<Release> {
        download::parse_releases(RECORDED, true, KEY).unwrap().0
    }

    fn indexed(release: &Release, source: bool) -> Indexed {
        Indexed {
            name: "uk-20260923".into(),
            path: "indexes/uk-20260923.ecl".into(),
            edition: "http://snomed.info/sct/83821000000107/version/20260923".into(),
            archive_sha256: release.archive_file_sha256.to_ascii_lowercase(),
            source: source.then(|| Source {
                distributor: "trud".into(),
                item: 1799,
                release_id: release.id.clone(),
                release_name: release.name.clone(),
                release_date: release.release_date.clone(),
                archive_file_name: release.archive_file_name.clone(),
            }),
        }
    }

    fn assert_safe(text: &str) {
        for forbidden in [KEY, "archiveFileUrl", "/keys/"] {
            assert!(!text.contains(forbidden), "unsafe output: {text}");
        }
    }

    fn outputs(assessment: &Assessment<'_>, names: &[String]) -> (Value, String) {
        let command = suggested_command(1799, assessment, names);
        let json = render_json(1799, assessment, command.as_deref(), KEY);
        let human = render_human(1799, assessment, command.as_deref(), KEY);
        assert_safe(&json);
        assert_safe(&human);
        (serde_json::from_str(&json).unwrap(), human)
    }

    #[test]
    fn current_archive_is_up_to_date_case_insensitively() {
        let releases = releases();
        let indexes = [indexed(&releases[0], true)];
        let assessment = assess(1799, &releases, &indexes);
        assert_eq!(assessment.status, Status::UpToDate);
        let (json, human) = outputs(&assessment, &[]);
        assert_eq!(json["status"], "up_to_date");
        assert_eq!(json["item"], 1799);
        assert_eq!(json["item_name"], "uk-monolith");
        assert_eq!(json["indexed"]["release_id"], releases[0].id);
        assert_eq!(json["latest"]["bytes"], 1234);
        assert!(json.get("command").is_none());
        assert!(json.get("releases_behind").is_none());
        assert!(human.contains("Status: up to date"));
    }

    #[test]
    fn behind_by_two_and_most_recent_indexed_release_wins() {
        let releases = releases();
        let mut indexes = vec![indexed(&releases[2], false)];
        let assessment = assess(1799, &releases, &indexes);
        assert_eq!(assessment.status, Status::Behind { releases_behind: 2 });
        let (json, human) = outputs(&assessment, &[]);
        assert_eq!(json["releases_behind"], 2);
        assert_eq!(json["indexed"]["release_name"], releases[2].name);
        assert!(human.contains("behind by 2 releases"));
        assert_eq!(
            json["command"],
            format!(
                "snomed-ecl-engine download uk-monolith --release {} --name uk-20260923",
                releases[0].id
            )
        );
        indexes.push(indexed(&releases[1], true));
        let assessment = assess(1799, &releases, &indexes);
        assert_eq!(assessment.status, Status::Behind { releases_behind: 1 });
        assert_eq!(assessment.indexed.unwrap().archive_sha256, "b".repeat(64));
        indexes.push(indexed(&releases[0], false));
        assert_eq!(assess(1799, &releases, &indexes).status, Status::UpToDate);
    }

    #[test]
    fn reissue_with_the_same_name_with_and_without_source() {
        let mut releases = releases();
        releases[1].name = releases[0].name.clone();
        for source in [false, true] {
            let indexes = [indexed(&releases[1], source)];
            let assessment = assess(1799, &releases, &indexes);
            assert_eq!(assessment.status, Status::Reissued);
            let (json, human) = outputs(&assessment, &["uk-20260923".into()]);
            assert_eq!(json["status"], "reissued");
            assert!(json["command"]
                .as_str()
                .unwrap()
                .ends_with("--name uk-20260923-9f8e7d6c"));
            assert!(human.contains("Status: reissued"));
        }
    }

    #[test]
    fn source_id_recognises_a_replaced_archive_absent_from_history() {
        let releases = releases();
        let mut index = indexed(&releases[0], true);
        index.archive_sha256 = "d".repeat(64);
        index.source.as_mut().unwrap().release_name = "Previous label".into();
        let indexes = [index];
        let assessment = assess(1799, &releases, &indexes);
        assert_eq!(assessment.status, Status::Reissued);
        outputs(&assessment, &[]);
        // An ID for an older release is insufficient to claim its hash matches.
        let mut index = indexed(&releases[1], true);
        index.archive_sha256 = "d".repeat(64);
        assert_eq!(assess(1799, &releases, &[index]).status, Status::Unknown);
    }

    #[test]
    fn unknown_archive_empty_library_and_no_releases() {
        let releases = releases();
        let mut index = indexed(&releases[0], false);
        index.archive_sha256 = "d".repeat(64);
        let indexes = [index];
        let assessment = assess(1799, &releases, &indexes);
        assert_eq!(assessment.status, Status::Unknown);
        let (json, _) = outputs(&assessment, &[]);
        assert_eq!(json["indexed"]["name"], "uk-20260923");
        assert!(json.get("command").is_some());
        let assessment = assess(1799, &releases, &[]);
        let (json, human) = outputs(&assessment, &[]);
        assert!(json["indexed"].is_null());
        assert!(human.contains("Indexed: none"));
        assert!(json["command"]
            .as_str()
            .unwrap()
            .ends_with("--name uk-20260923"));
        let assessment = assess(1799, &[], &indexes);
        let (json, _) = outputs(&assessment, &[]);
        assert!(json["latest"].is_null());
        assert!(json.get("command").is_none());
    }

    #[test]
    fn unknown_uses_the_items_family_and_latest_edition_date() {
        let releases = releases();
        let mut uk_old = indexed(&releases[0], false);
        uk_old.name = "uk-old".into();
        uk_old.archive_sha256 = "d".repeat(64);
        uk_old.edition = "http://snomed.info/sct/83821000000107/version/20260101".into();
        let mut uk_new = indexed(&releases[0], false);
        uk_new.archive_sha256 = "e".repeat(64);
        let mut international = indexed(&releases[0], false);
        international.archive_sha256 = "f".repeat(64);
        international.edition = "http://snomed.info/sct/900000000000207008/version/20261201".into();
        let mut indexes = [uk_old, uk_new, international];
        let assessment = assess(1799, &releases, &indexes);
        assert_eq!(assessment.status, Status::Unknown);
        assert_eq!(assessment.indexed.unwrap().name, "uk-20260923");
        let (json, _) = outputs(&assessment, &[]);
        assert!(json["command"]
            .as_str()
            .unwrap()
            .ends_with("--name uk-20260923"));
        let assessment = assess(1799, &releases, &indexes[2..]);
        assert!(assessment.indexed.is_none());
        outputs(&assessment, &[]);

        // For an unnamed item, edition dates decide even across module IDs.
        let assessment = assess(101, &releases, &indexes);
        assert_eq!(assessment.indexed.unwrap().edition, indexes[2].edition);
        let assessment = assess(101, &releases, &indexes[..2]);
        assert_eq!(assessment.indexed.unwrap().name, "uk-20260923");
        indexes[2].edition = "http://snomed.info/sct/900000000000207008/version/20251201".into();
        let assessment = assess(101, &releases, &indexes);
        assert_eq!(assessment.indexed.unwrap().name, "uk-20260923");
    }

    #[test]
    fn invalid_history_is_skipped_but_invalid_newest_fails() {
        let mut body: Value = serde_json::from_str(RECORDED).unwrap();
        body["releases"][1]["archiveFileSha256"] = json!("");
        let (releases, warnings) = download::parse_releases(&body.to_string(), true, KEY).unwrap();
        assert_eq!(releases.len(), 2);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("skipped historical TRUD release 2"));
        assert_safe(&warnings[0]);
        assert_safe(
            &download::parse_releases(&body.to_string(), false, KEY)
                .err()
                .unwrap()
                .to_string(),
        );
        body["releases"][0]["archiveFileSha256"] = json!("");
        assert_safe(
            &download::parse_releases(&body.to_string(), true, KEY)
                .err()
                .unwrap()
                .to_string(),
        );
        assert_safe(
            &download::parse_releases("{archiveFileUrl: SENTINEL-KEY-123}", true, KEY)
                .err()
                .unwrap()
                .to_string(),
        );
    }

    #[test]
    fn null_or_missing_history_fields_are_skipped_but_newest_fails() {
        for missing in [false, true] {
            let mut body: Value = serde_json::from_str(RECORDED).unwrap();
            for position in [1, 0] {
                if missing {
                    body["releases"][position]
                        .as_object_mut()
                        .unwrap()
                        .remove("archiveFileSha256");
                } else {
                    body["releases"][position]["archiveFileSha256"] = Value::Null;
                }
                if position == 1 {
                    let (releases, warnings) =
                        download::parse_releases(&body.to_string(), true, KEY).unwrap();
                    assert_eq!(releases.len(), 2);
                    assert_eq!(releases[0].id, body["releases"][0]["id"]);
                    assert_eq!(releases[1].id, body["releases"][2]["id"]);
                    assert_eq!(
                        warnings,
                        ["Warning: skipped historical TRUD release 2: TRUD returned release metadata this version cannot read"]
                    );
                    assert_safe(&warnings[0]);
                    assert!(!warnings[0].contains("https://"));
                }
                let error = download::parse_releases(&body.to_string(), false, KEY)
                    .err()
                    .unwrap();
                assert_safe(&error.to_string());
            }
            let error = download::parse_releases(&body.to_string(), true, KEY)
                .err()
                .unwrap();
            assert_safe(&error.to_string());
        }
    }

    #[test]
    fn names_use_edition_family_archive_date_and_collision_suffix() {
        let mut releases = releases();
        let mut index = indexed(&releases[0], false);
        index.edition = "http://snomed.info/sct/900000000000207008/version/20260101".into();
        assert_eq!(
            suggested_name(1799, &releases[0], Some(&index), &[]).as_deref(),
            Some("uk-20260923")
        );
        assert_eq!(
            suggested_name(1799, &releases[0], None, &["uk-20260923".into()]).as_deref(),
            Some("uk-20260923-9f8e7d6c")
        );
        assert_eq!(suggested_name(101, &releases[0], None, &[]), None);
        releases[0].id = "synthetic_123_more_20261101000001Z.zip".into();
        assert_eq!(
            suggested_name(1799, &releases[0], None, &[]).as_deref(),
            Some("uk-20261101")
        );
        releases[0].id = "synthetic.zip".into();
        assert_eq!(
            suggested_name(1799, &releases[0], None, &[]).as_deref(),
            Some("uk-20260923")
        );
        releases[0].archive_file_name = "synthetic.zip".into();
        assert_eq!(suggested_name(1799, &releases[0], None, &[]), None);
    }

    #[test]
    fn commands_quote_shell_values_and_validate_release_ids() {
        let mut releases = releases();
        releases[0].id = "synthetic_'quote $(printf bad)_20260923.zip".into();
        let assessment = assess(1799, &releases, &[]);
        let command = suggested_command(101, &assessment, &[]).unwrap();
        assert_eq!(command, "snomed-ecl-engine download 101 --release 'synthetic_'\"'\"'quote $(printf bad)_20260923.zip'");
        assert_eq!(shell_quote("safe-1.0_name"), "safe-1.0_name");
        assert_eq!(shell_quote(""), "''");
        outputs(&assessment, &[]);
        for id in ["../synthetic.zip", "synthetic.txt", "a/synthetic.zip"] {
            releases[0].id = id.into();
            let assessment = assess(1799, &releases, &[]);
            assert!(suggested_command(1799, &assessment, &[]).is_none());
            let (json, _) = outputs(&assessment, &[]);
            assert_eq!(json["status"], "unknown");
            assert!(json.get("command").is_none());
        }
    }

    #[test]
    fn redaction_covers_errors_and_all_rendered_metadata() {
        let mut releases = releases();
        let mut index = indexed(&releases[0], true);
        let unsafe_text = format!("{KEY} https://isd.digital.nhs.uk/keys/{KEY}/archiveFileUrl");
        releases[0].name = unsafe_text.clone();
        releases[0].release_date = unsafe_text.clone();
        index.name = unsafe_text.clone();
        index.path = unsafe_text.clone().into();
        index.source.as_mut().unwrap().release_id = unsafe_text.clone();
        index.source.as_mut().unwrap().release_name = unsafe_text.clone();
        let indexes = [index];
        outputs(&assess(1799, &releases, &indexes), &[]);
        for error in [
            unsafe_text,
            format!("http status: 403 GET https://isd.digital.nhs.uk/keys/{KEY}/items/1799"),
        ] {
            assert_safe(&download::redact(&error, KEY));
        }
        assert!(download::redact("http status: 403", KEY).contains("subscribed"));
        assert_safe(&render_json(
            1799,
            &assess(1799, &releases, &[]),
            Some(KEY),
            KEY,
        ));
        assert_safe(&render_human(
            1799,
            &assess(1799, &releases, &[]),
            Some(KEY),
            KEY,
        ));
    }

    #[test]
    fn rendered_paths_and_hashes_survive_redaction_with_a_short_key() {
        let releases = releases();
        let mut index = indexed(&releases[0], false);
        index.path = "/home/u/keys/uk.ecl".into();
        index.archive_sha256 = "a".repeat(64);
        let indexes = [index];
        let assessment = assess(1799, &releases, &indexes);
        let json: Value = serde_json::from_str(&render_json(1799, &assessment, None, "a")).unwrap();
        assert_eq!(json["indexed"]["path"], "/home/u/keys/uk.ecl");
        assert_eq!(json["indexed"]["archive_sha256"], "a".repeat(64));
        assert_eq!(json["latest"]["sha256"], releases[0].archive_file_sha256);
        assert!(render_human(1799, &assessment, None, "a").contains("SHA-256 aaaaaaaa"));
    }

    #[test]
    fn options_default_items_and_exit_codes() {
        let args = [
            "101",
            "uk-monolith",
            "1799",
            "--index",
            "one.ecl",
            "--index",
            "two.ecl",
            "--index",
            "three.ecl",
            "--exit-code",
        ]
        .map(str::to_owned);
        let options = Options::parse(&args).unwrap();
        assert_eq!(options.items, [101, 1799]);
        assert_eq!(
            options.indexes,
            [
                PathBuf::from("one.ecl"),
                PathBuf::from("two.ecl"),
                PathBuf::from("three.ecl")
            ]
        );
        assert!(options.exit_code);
        let options =
            Options::parse(&["--index", "a.ecl", "uk-monolith"].map(str::to_owned)).unwrap();
        assert_eq!(options.indexes, [PathBuf::from("a.ecl")]);
        assert_eq!(options.items, [1799]);
        assert!(Options::parse(&["--index".into()]).is_err());
        assert!(Options::parse(&["--index".into(), "--exit-code".into()]).is_err());
        assert!(Options::parse(&["--api-key".into(), KEY.into()]).is_err());
        assert_eq!(default_items(&[]), [1799]);
        let releases = releases();
        let mut indexes = vec![indexed(&releases[0], true), indexed(&releases[1], true)];
        assert_eq!(default_items(&indexes), [1799]);
        indexes[0].source.as_mut().unwrap().item = 101;
        assert_eq!(default_items(&indexes), [101, 1799]);
        assert_eq!(exit_code(&[Status::UpToDate]), 0);
        assert_eq!(exit_code(&[Status::UpToDate, Status::Unknown]), 4);
        assert_eq!(
            exit_code(&[Status::Unknown, Status::Behind { releases_behind: 2 }]),
            3
        );
        assert_eq!(exit_code(&[Status::Unknown, Status::Reissued]), 3);
    }

    #[test]
    fn explicit_indexes_supply_default_items_instead_of_the_library() {
        use sha2::{Digest, Sha256};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path();
        let core = b"synthetic core";
        std::fs::write(path.join("core.bin"), core).unwrap();
        let mut manifest = json!({
            "format": 1, "edition": "synthetic:edition", "archive_sha256": "a".repeat(64),
            "source": {
                "distributor": "trud", "item": 101, "release_id": "synthetic.zip",
                "release_name": "Synthetic release", "release_date": "2026-01-01",
                "archive_file_name": "synthetic.zip"
            },
            "concept_count": 0, "active_concept_count": 0, "hierarchy_edges": 0,
            "attributes": 0, "concrete_attributes": 0, "concrete_values": 0,
            "core_bytes": core.len(), "core_sha256": format!("{:x}", Sha256::digest(core)),
            "display_bytes": 0, "display_sha256": "",
            "display_refsets": [], "displays_selected": 0, "module_dependencies": [],
            "capabilities": []
        });
        std::fs::write(path.join("manifest.json"), manifest.to_string()).unwrap();
        let args = ["--index".into(), path.to_string_lossy().into_owned()];
        let mut options = Options::parse(&args).unwrap();
        let indexes = options
            .load_indexes(std::iter::once(Path::new("unread-library.ecl")))
            .unwrap();
        assert_eq!(indexes.len(), 1);
        assert_eq!(options.items, [101]);

        let mut options = Options::parse(&[]).unwrap();
        options.load_indexes(std::iter::once(path)).unwrap();
        assert_eq!(options.items, [101]);

        let mut options =
            Options::parse(&["1799".into(), args[0].clone(), args[1].clone()]).unwrap();
        options.load_indexes(std::iter::empty()).unwrap();
        assert_eq!(options.items, [1799]);

        manifest["source"] = Value::Null;
        std::fs::write(path.join("manifest.json"), manifest.to_string()).unwrap();
        let mut options = Options::parse(&args).unwrap();
        options.load_indexes(std::iter::empty()).unwrap();
        assert_eq!(options.items, [1799]);
    }
}
