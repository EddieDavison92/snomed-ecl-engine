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
pub const ITEMS: &[(&str, u32, &str)] = &[(
    "uk-monolith",
    1799,
    "SNOMED CT UK Monolith Edition, RF2: Snapshot",
)];

#[derive(Debug, Deserialize)]
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
struct Releases {
    releases: Vec<Release>,
}

/// The TRUD item number for a name such as `uk-monolith`, or a number.
pub fn item(name: &str) -> Result<u32> {
    if let Some((_, number, _)) = ITEMS.iter().find(|(known, _, _)| *known == name) {
        return Ok(*number);
    }
    name.parse().map_err(|_| {
        let known: Vec<_> = ITEMS.iter().map(|(known, _, _)| *known).collect();
        anyhow!(
            "Unknown TRUD item {name}. Give an item number, or one of: {}",
            known.join(", ")
        )
    })
}

fn key() -> Result<String> {
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
        .timeout_connect(Some(Duration::from_secs(60)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_recv_body(Some(body))
        .build()
        .into()
}

/// An error's text with the API key removed. TRUD answers a bad key, or an
/// item the account is not subscribed to, with a 4xx status, so that says so.
fn redact(error: impl std::fmt::Display, key: &str) -> String {
    let text = error.to_string().replace(key, "***");
    if [
        "http status: 400",
        "http status: 401",
        "http status: 403",
        "http status: 404",
    ]
    .iter()
    .any(|status| text.contains(status))
    {
        format!("{text}. Check {ENV_KEY}, and that your TRUD account is subscribed to this item")
    } else {
        text
    }
}

/// Checks a release's metadata before anything is fetched from it.
fn check(release: &Release) -> Result<()> {
    let name = &release.archive_file_name;
    ensure!(
        Path::new(name)
            .file_name()
            .is_some_and(|n| n == name.as_str())
            && name.ends_with(".zip"),
        "TRUD named an unexpected archive file"
    );
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
    let url = format!(
        "{API}/{key}/items/{item}/releases{}",
        if latest { "?latest" } else { "" }
    );
    let body = agent(std::time::Duration::from_secs(120))
        .get(&url)
        .call()
        .map_err(|error| anyhow!("TRUD release lookup failed: {}", redact(error, &key)))?
        .into_body()
        .read_to_string()
        .map_err(|error| anyhow!("TRUD release lookup failed: {}", redact(error, &key)))?;
    let parsed: Releases =
        serde_json::from_str(&body).context("TRUD returned a response this version cannot read")?;
    for release in &parsed.releases {
        check(release)?;
    }
    Ok(parsed.releases)
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
        assert_eq!(item("101").unwrap(), 101);
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
        let text = redact("GET https://host/keys/secret123/items failed", "secret123");
        assert!(!text.contains("secret123"));
        assert!(text.contains("***"));
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
