//! Downloads RF2 releases from NHS England's TRUD service.
//!
//! TRUD puts the API key in every request path, including download links, so
//! the key is never printed, and it is removed from any error before that
//! error is shown. TRUD also publishes each archive's SHA-256, which is the
//! distributor's checksum `add` needs, so a download is verified against it.
use anyhow::{anyhow, bail, ensure, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

pub const ENV_KEY: &str = "TRUD_API_KEY";
const API: &str = "https://isd.digital.nhs.uk/trud/api/v1/keys";

/// TRUD items known by name. Any other item is given by its number.
pub struct Item {
    pub name: &'static str,
    pub number: u32,
    pub description: &'static str,
    pub edition_module: u64,
}

pub const ITEMS: &[Item] = &[
    Item {
        name: "uk-monolith",
        number: 1799,
        description: "SNOMED CT UK Monolith Edition, RF2: Snapshot",
        edition_module: 83821000000107,
    },
    Item {
        name: "international",
        number: 4,
        description: "SNOMED CT International Edition, RF2",
        edition_module: 900000000000207008,
    },
];

/// Known edition module, including when a known item was given by number.
pub fn expected_module(number: u32) -> Option<u64> {
    ITEMS
        .iter()
        .find(|item| item.number == number)
        .map(|item| item.edition_module)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub id: String,
    pub name: String,
    pub release_date: String,
    archive_file_url: String,
    pub archive_file_name: String,
    pub archive_file_size_bytes: u64,
    pub archive_file_sha256: String,
}

#[derive(Deserialize)]
struct Releases<T = Release> {
    releases: Vec<T>,
}

/// The TRUD item number for a name such as `uk-monolith`, or a number.
pub fn item(name: &str) -> Result<u32> {
    if let Some(item) = ITEMS.iter().find(|item| item.name == name) {
        return Ok(item.number);
    }
    name.parse().map_err(|_| {
        let known: Vec<_> = ITEMS
            .iter()
            .map(|item| format!("{} ({})", item.name, item.description))
            .collect();
        anyhow!(
            "Unknown TRUD item {name}. Give an item number, or one of: {}",
            known.join(", ")
        )
    })
}

pub fn key() -> Result<String> {
    std::env::var(ENV_KEY)
        .ok()
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .with_context(|| {
            format!(
                "Set {ENV_KEY} to your TRUD API key. Register at \
                 https://isd.digital.nhs.uk/trud/, subscribe to the item, and copy the key \
                 from your account page"
            )
        })
}

/// An HTTP agent that gives up on a stalled server. Connecting and the first
/// response byte get a minute each; `body` bounds reading the whole reply,
/// which for an archive must allow a slow connection to finish.
fn agent(body: std::time::Duration) -> ureq::Agent {
    use std::time::Duration;
    ureq::Agent::config_builder()
        // The key is in every URL, so no hop, redirects included, may use plain HTTP.
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(60)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_recv_body(Some(body))
        .build()
        .into()
}

/// An error's text with the API key removed. TRUD answers a bad key, or an
/// item the account is not subscribed to, with a 4xx status, so that says so.
pub fn redact(error: impl std::fmt::Display, key: &str) -> String {
    let original = error.to_string();
    let mut text = String::new();
    // Remove credential-bearing TRUD URLs, including keys other than the one
    // configured locally. A /keys/ directory in a file path is ordinary data.
    for part in original.split_inclusive(char::is_whitespace) {
        let trud_url = part
            .split("https://isd.digital.nhs.uk/")
            .skip(1)
            .any(|rest| {
                let url = rest.split(['\'', '"', '<', '>']).next().unwrap_or_default();
                url.strip_prefix("keys/")
                    .or_else(|| url.split_once("/keys/").map(|(_, path)| path))
                    .is_some_and(|path| {
                        path.split_once('/').is_some_and(|(key, _)| !key.is_empty())
                    })
            });
        if trud_url {
            text.push_str("[redacted TRUD metadata]");
            text.push_str(&part[part.trim_end_matches(char::is_whitespace).len()..]);
        } else {
            text.push_str(part);
        }
    }
    // The eight-character threshold keeps short keys from corrupting ordinary paths and hashes.
    if key.chars().count() >= 8 {
        text = text.replace(key, "***");
    }
    if [
        "http status: 400",
        "http status: 401",
        "http status: 403",
        "http status: 404",
    ]
    .iter()
    .any(|status| text.contains(status))
        && !text.contains(&format!("Check {ENV_KEY}"))
    {
        format!("{text}. Check {ENV_KEY}, and that your TRUD account is subscribed to this item")
    } else {
        text
    }
}

/// The archive file-name rules also used for IDs in suggested commands.
pub fn check_file_name(name: &str) -> Result<()> {
    ensure!(
        Path::new(name).file_name().is_some_and(|n| n == name) && name.ends_with(".zip"),
        "TRUD named an unexpected archive file"
    );
    Ok(())
}

/// Checks a release's metadata before anything is fetched from it.
fn check(release: &Release) -> Result<()> {
    check_file_name(&release.archive_file_name)?;
    let name = &release.archive_file_name;
    ensure!(
        release
            .archive_file_url
            .starts_with("https://isd.digital.nhs.uk/"),
        "TRUD offered a download from an unexpected host"
    );
    ensure!(
        release.archive_file_sha256.len() == 64
            && release
                .archive_file_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit()),
        "TRUD gave no SHA-256 for {name}"
    );
    Ok(())
}

/// The item's releases, newest first. With `latest`, only the newest.
pub fn releases(item: u32, latest: bool) -> Result<Vec<Release>> {
    let key = key()?;
    lookup(item, latest, &key)
        .and_then(|body| parse_releases(&body, false, &key).map(|(releases, _)| releases))
        .map_err(|error| anyhow!("{}", redact(format!("{error:#}"), &key)))
}

/// A requested release is validated on its own; unrelated invalid metadata
/// must not prevent downloading an archive recommended by an update check.
pub fn selected_release(item: u32, id: &str) -> Result<Release> {
    let key = key()?;
    lookup(item, false, &key)
        .and_then(|body| parse_selected_release(&body, id))
        .map_err(|error| anyhow!("{}", redact(format!("{error:#}"), &key)))
}

fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<Releases<T>> {
    serde_json::from_str(body)
        .map_err(|_| anyhow!("TRUD returned a response this version cannot read"))
}

fn parse_selected_release(body: &str, id: &str) -> Result<Release> {
    let metadata = parse::<serde_json::Value>(body)?
        .releases
        .into_iter()
        .find(|release| release.get("id").and_then(serde_json::Value::as_str) == Some(id))
        .with_context(|| format!("TRUD has no release {id}; `download --list` shows them"))?;
    let release: Release = serde_json::from_value(metadata)
        .map_err(|_| anyhow!("TRUD returned release metadata this version cannot read"))?;
    check(&release)?;
    Ok(release)
}

/// Update checks can still use the recent history when an older release has
/// incomplete metadata. The newest release must always pass validation.
pub fn releases_for_updates(item: u32, key: &str) -> Result<Vec<Release>> {
    let result = lookup(item, false, key).and_then(|body| parse_releases(&body, true, key));
    let (releases, warnings) =
        result.map_err(|error| anyhow!("{}", redact(format!("{error:#}"), key)))?;
    for warning in warnings {
        eprintln!("{}", crate::presentation::clean_message(&warning));
    }
    Ok(releases)
}

fn lookup(item: u32, latest: bool, key: &str) -> Result<String> {
    let url = format!(
        "{API}/{key}/items/{item}/releases{}",
        if latest { "?latest" } else { "" }
    );
    agent(std::time::Duration::from_secs(120))
        .get(&url)
        .call()
        .map_err(|error| anyhow!("TRUD release lookup failed: {}", redact(error, key)))?
        .into_body()
        .read_to_string()
        .map_err(|error| anyhow!("TRUD release lookup failed: {}", redact(error, key)))
}

pub(crate) fn parse_releases(
    body: &str,
    lenient: bool,
    key: &str,
) -> Result<(Vec<Release>, Vec<String>)> {
    let parsed = parse::<serde_json::Value>(body)?;
    let mut releases = Vec::new();
    let mut warnings = Vec::new();
    for (position, metadata) in parsed.releases.into_iter().enumerate() {
        let release = serde_json::from_value(metadata)
            .map_err(|_| anyhow!("TRUD returned release metadata this version cannot read"))
            .and_then(|release| {
                check(&release)?;
                Ok(release)
            });
        match release {
            Ok(release) => releases.push(release),
            Err(error) if lenient && position > 0 => warnings.push(format!(
                "Warning: skipped historical TRUD release {}: {}",
                position + 1,
                redact(error, key)
            )),
            Err(error) => return Err(anyhow!("{}", redact(error, key))),
        }
    }
    Ok((releases, warnings))
}

/// Downloads a release into `folder`, verifying its size and SHA-256 against
/// TRUD's metadata. A complete, matching copy already there is reused.
pub fn fetch(release: &Release, folder: &Path) -> Result<PathBuf> {
    let path = folder.join(&release.archive_file_name);
    if path.is_file() && hash_file(&path)?.eq_ignore_ascii_case(&release.archive_file_sha256) {
        eprintln!("  Using {}, already downloaded", release.archive_file_name);
        return Ok(path);
    }
    std::fs::create_dir_all(folder)
        .with_context(|| format!("Cannot create {}", folder.display()))?;
    let key = key()?;
    let partial = path.with_extension("zip.partial");
    let result = download(release, &partial, &key);
    if let Err(error) = result {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    std::fs::rename(&partial, &path)
        .with_context(|| format!("Cannot move the download to {}", path.display()))?;
    Ok(path)
}

fn download(release: &Release, partial: &Path, key: &str) -> Result<()> {
    // Four hours covers a 600 MB archive at a slow 350 kbit/s.
    let response = agent(std::time::Duration::from_secs(4 * 3600))
        .get(&release.archive_file_url)
        .call()
        .map_err(|error| anyhow!("TRUD download failed: {}", redact(error, key)))?;
    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(partial)
        .with_context(|| format!("Cannot write {}", partial.display()))?;
    let total = release.archive_file_size_bytes;
    let mut hash = Sha256::new();
    let mut written = 0u64;
    let mut shown = u64::MAX;
    let mut buffer = vec![0; 1 << 20];
    let live = std::io::stderr().is_terminal();
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|error| anyhow!("TRUD download failed: {}", redact(error, key)))?;
        if n == 0 {
            break;
        }
        // Stop before writing more than TRUD said the archive holds.
        ensure!(
            written + n as u64 <= total,
            "The download is larger than the {total} bytes TRUD published; stopped"
        );
        file.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
        written += n as u64;
        // Every percent on a terminal, every tenth otherwise.
        let percent = written.saturating_mul(100) / total.max(1);
        let step = if live { percent } else { percent / 10 * 10 };
        if step != shown {
            shown = step;
            let line = format!(
                "  Downloading {}  {percent}% of {:.0} MiB",
                release.archive_file_name,
                total as f64 / 1_048_576.0
            );
            if live {
                eprint!("\r{line}");
            } else {
                eprintln!("{line}");
            }
        }
    }
    if live {
        eprintln!();
    }
    file.flush()?;
    file.sync_all()?;
    ensure!(
        written == total,
        "Download ended at {written} of {total} bytes; try again"
    );
    let actual = format!("{:x}", hash.finalize());
    if !actual.eq_ignore_ascii_case(&release.archive_file_sha256) {
        bail!(
            "The download's SHA-256 is {actual}, not the {} TRUD published; try again",
            release.archive_file_sha256
        );
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    snomed_ecl_engine::store::sha256(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(name: &str, url: &str, sha256: &str) -> Release {
        Release {
            id: "r".into(),
            name: "Release".into(),
            release_date: "2026-09-02".into(),
            archive_file_url: url.into(),
            archive_file_name: name.into(),
            archive_file_size_bytes: 1,
            archive_file_sha256: sha256.into(),
        }
    }

    #[test]
    fn items_resolve_by_name_or_number() {
        assert_eq!(item("uk-monolith").unwrap(), 1799);
        assert_eq!(item("international").unwrap(), 4);
        assert_eq!(item("4").unwrap(), 4);
        assert_eq!(item("101").unwrap(), 101);
        assert_eq!(expected_module(1799), Some(83821000000107));
        assert_eq!(expected_module(4), Some(900000000000207008));
        assert_eq!(expected_module(101), None);
        assert!(item("uk-drug-extension").is_err());
    }

    #[test]
    fn release_metadata_is_checked_before_use() {
        let hash = "a".repeat(64);
        let good = "https://isd.digital.nhs.uk/download/x.zip";
        assert!(check(&release("uk.zip", good, &hash)).is_ok());
        assert!(check(&release("../uk.zip", good, &hash)).is_err());
        assert!(check(&release("uk.exe", good, &hash)).is_err());
        assert!(check(&release("uk.zip", "https://example.com/uk.zip", &hash)).is_err());
        assert!(check(&release("uk.zip", good, "abc")).is_err());
    }

    #[test]
    fn errors_never_carry_the_key() {
        for url in [
            "https://isd.digital.nhs.uk/trud/api/v1/keys/secret123/items/1799",
            "https://isd.digital.nhs.uk/download/api/v1/keys/other-key/content/x.zip",
            "https://isd.digital.nhs.uk/keys/k/items/1799",
        ] {
            let text = redact(format!("GET {url} failed"), "secret123");
            assert_eq!(text, "GET [redacted TRUD metadata] failed");
        }
        assert_eq!(redact("Key: secret123", "secret123"), "Key: ***");
    }

    #[test]
    fn redaction_preserves_paths_and_short_keys_in_data() {
        let data = "/home/u/keys/uk.ecl archiveFileUrl a1234567 /data/a.ecl";
        for key in ["", "a", "1234567", "unrelated-key"] {
            assert_eq!(redact(data, key), data);
        }
        assert_eq!(
            redact("https://example.com/keys/other/value", "a"),
            "https://example.com/keys/other/value"
        );
        assert_eq!(redact("/data/secret123.ecl", "secret123"), "/data/***.ecl");
        assert_eq!(
            redact(
                "GET https://isd.digital.nhs.uk/trud/api/v1/keys/a/items/1799 failed",
                "a"
            ),
            "GET [redacted TRUD metadata] failed"
        );
    }

    #[test]
    fn selected_release_validates_only_the_requested_metadata() {
        let mut body: serde_json::Value =
            serde_json::from_str(include_str!("updates/test-releases.json")).unwrap();
        let latest = body["releases"][0]["id"].as_str().unwrap().to_owned();
        let older = body["releases"][1]["id"].as_str().unwrap().to_owned();
        body["releases"][1]["archiveFileSha256"] = "".into();
        assert_eq!(
            parse_selected_release(&body.to_string(), &latest)
                .unwrap()
                .id,
            latest
        );
        assert!(parse_selected_release(&body.to_string(), &older).is_err());
        body["releases"][0]["archiveFileSha256"] = "".into();
        body["releases"][1]["archiveFileSha256"] = "b".repeat(64).into();
        assert_eq!(
            parse_selected_release(&body.to_string(), &older)
                .unwrap()
                .id,
            older
        );
        assert!(parse_releases(&body.to_string(), true, "synthetic-key").is_err());
        assert!(parse_selected_release(&body.to_string(), "missing.zip").is_err());
        body["releases"][0]
            .as_object_mut()
            .unwrap()
            .remove("archiveFileName");
        assert!(parse_selected_release(&body.to_string(), &older).is_ok());
        assert!(parse_selected_release(&body.to_string(), &latest).is_err());
    }

    #[test]
    fn trud_responses_parse() {
        let body = r#"{"apiVersion":"1","releases":[{"id":"uk_sct2mo_42.5.0_20260826000001Z.zip",
            "name":"Release 42.5.0","releaseDate":"2026-09-02",
            "archiveFileUrl":"https://isd.digital.nhs.uk/download/api/v1/keys/k/content/items/1799/x.zip",
            "archiveFileName":"uk_sct2mo_42.5.0_20260826000001Z.zip","archiveFileSizeBytes":609629807,
            "archiveFileSha1":"00","archiveFileSha256":"1330D2F2F48022D2594306F8DBBD891F1709E639E91CB97B281A9E796CFEDD2B"}],
            "httpStatus":200,"message":"OK"}"#;
        let parsed: Releases = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.releases.len(), 1);
        assert!(check(&parsed.releases[0]).is_ok());
        assert_eq!(parsed.releases[0].archive_file_size_bytes, 609_629_807);
    }
}
