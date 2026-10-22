//! CLI-only release checks. No HTTP or installation policy enters the library.
use anyhow::{bail, ensure, Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REPO: &str = "EddieDavison92/snomed-ecl-engine";
const DAY: u64 = 24 * 60 * 60;
const MAX_ARCHIVE: u64 = 128 * 1024 * 1024;
const MAX_BINARY: u64 = 256 * 1024 * 1024;
const CI_ENV: &[&str] = &[
    "CI",
    "GITHUB_ACTIONS",
    "GITLAB_CI",
    "TF_BUILD",
    "JENKINS_URL",
    "BUILD_ID",
    "BUILD_NUMBER",
    "TEAMCITY_VERSION",
    "CIRCLECI",
    "TRAVIS",
    "BUILDKITE",
    "DRONE",
    "APPVEYOR",
    "CODEBUILD_BUILD_ID",
    "BITBUCKET_BUILD_NUMBER",
];

fn version(value: &str) -> Result<Version> {
    Version::parse(value.strip_prefix('v').unwrap_or(value)).context("Invalid release version")
}

fn newer(candidate: &str, current: &str) -> Result<bool> {
    Ok(version(candidate)?
        .cmp_precedence(&version(current)?)
        .is_gt())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstallMethod {
    Npm,
    Cargo,
    Homebrew,
    Container,
    Standalone,
}

impl InstallMethod {
    fn command(self, release: &Version) -> String {
        match self {
            Self::Npm => "npm i -g snomed-ecl-engine".into(),
            Self::Cargo => "cargo install snomed-ecl-engine".into(),
            Self::Homebrew => "brew upgrade snomed-ecl-engine".into(),
            Self::Container => {
                format!("docker pull ghcr.io/eddiedavison92/snomed-ecl-engine:{release}")
            }
            Self::Standalone => "snomed-ecl-engine self-update".into(),
        }
    }
}

fn detect_install(
    exe: &Path,
    cargo_home: Option<&Path>,
    marker: Option<&str>,
    container: bool,
) -> InstallMethod {
    let path = exe
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if marker == Some("npm") || path.split('/').any(|part| part == "node_modules") {
        InstallMethod::Npm
    } else if path.contains("/cellar/snomed-ecl-engine/")
        || path.contains("/opt/snomed-ecl-engine/")
    {
        InstallMethod::Homebrew
    } else if marker == Some("cargo")
        || cargo_home.is_some_and(|home| exe.parent() == Some(home.join("bin").as_path()))
        || path.contains("/.cargo/bin/")
    {
        InstallMethod::Cargo
    } else if marker == Some("container") || container {
        InstallMethod::Container
    } else {
        InstallMethod::Standalone
    }
}

fn install_method(exe: &Path) -> InstallMethod {
    let exe = fs::canonicalize(exe).unwrap_or_else(|_| exe.to_owned());
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .map(|home| fs::canonicalize(&home).unwrap_or(home));
    // cargo install --root records a receipt beside bin even when CARGO_HOME
    // is unset on later runs. Protect those installations too.
    let receipt = exe
        .parent()
        .filter(|parent| parent.file_name().is_some_and(|name| name == "bin"))
        .and_then(Path::parent)
        .is_some_and(|root| {
            root.join(".crates.toml").is_file() || root.join(".crates2.json").is_file()
        });
    let marker = std::env::var("SNOMED_ECL_INSTALL_METHOD").ok();
    let marker = marker.as_deref().or(receipt.then_some("cargo"));
    detect_install(
        &exe,
        cargo_home.as_deref(),
        marker,
        Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists(),
    )
}

fn suppressed(args: &[String], tty: bool, env: impl Fn(&str) -> Option<String>) -> bool {
    !tty || args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--json" | "--plain" | "self-update"))
        || env("SNOMED_ECL_NO_UPDATE_CHECK").as_deref() == Some("1")
        || CI_ENV.iter().any(|key| env(key).is_some())
}

#[derive(Default, Deserialize, Serialize)]
struct Cache {
    checked_at: u64,
    latest: Option<String>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn fresh(checked_at: u64, time: u64) -> bool {
    // A clock moving backwards cannot trigger repeated network requests.
    time.saturating_sub(checked_at) < DAY
}

fn cache_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    }?;
    Some(base.join("snomed-ecl-engine/cli-update.json"))
}

fn read_cache(path: &Path) -> Cache {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_cache(path: &Path, cache: &Cache) -> Result<()> {
    let parent = path.parent().context("Cache path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut temp, cache)?;
    temp.persist(path)?;
    Ok(())
}

/// Reserve today's attempt before starting HTTP, including failed attempts.
/// The exclusive file also stops simultaneous CLI processes checking twice.
fn reserve(path: &Path, time: u64) -> Result<bool> {
    fs::create_dir_all(path.parent().context("Cache path has no parent")?)?;
    // Keep this file: unlinking a held lock would allow a second inode to be
    // locked concurrently. OS locks are released even when a process exits.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    if file.try_lock().is_err() {
        return Ok(false);
    }
    let mut cache = read_cache(path);
    if fresh(cache.checked_at, time) {
        return Ok(false);
    }
    cache.checked_at = time;
    write_cache(path, &cache)?;
    Ok(true)
}

fn notice(latest: &str, method: InstallMethod) -> Option<String> {
    if !newer(latest, env!("CARGO_PKG_VERSION")).ok()? {
        return None;
    }
    let release = version(latest).ok()?;
    Some(format!(
        "snomed-ecl-engine {release} is available; upgrade: {}",
        method.command(&release)
    ))
}

fn print_notice(text: &str) {
    let _ = writeln!(io::stderr().lock(), "{text}");
}

pub fn startup(args: &[String]) {
    if suppressed(args, io::stderr().is_terminal(), |key| {
        std::env::var(key).ok()
    }) {
        return;
    }
    let Some(path) = cache_path() else {
        return;
    };
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let method = install_method(&exe);
    let _ = schedule_check(path, method, now(), print_notice, |_, _| {
        // The worker survives a fast CLI command and has no terminal output.
        let mut worker = std::process::Command::new(exe);
        worker
            .arg("__update-cache")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            worker.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        if let Ok(mut child) = worker.spawn() {
            // Reap it in long-lived sessions without holding up command exit.
            let _ = std::thread::Builder::new()
                .name("cli-update-reaper".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
    });
}

/// Private worker entry point: called before ordinary command parsing so it
/// cannot start another worker. All output and failures stay silent.
pub fn worker(args: &[String]) -> bool {
    if args != ["__update-cache"] {
        return false;
    }
    if let Some(path) = cache_path() {
        let time = read_cache(&path).checked_at;
        if time != 0 && fresh(time, now()) {
            refresh_cache(&path, time, &Client::new(Duration::from_secs(2)));
        }
    }
    true
}

fn refresh_cache(path: &Path, time: u64, client: &Client) {
    if let Ok(release) = client.release(None) {
        let _ = write_cache(
            path,
            &Cache {
                checked_at: time,
                latest: Some(release.tag_name),
            },
        );
    }
}

fn schedule_check<T>(
    path: PathBuf,
    method: InstallMethod,
    time: u64,
    output: impl Fn(&str),
    launch: impl FnOnce(PathBuf, u64) -> T,
) -> Option<T> {
    let cache = read_cache(&path);
    if let Some(text) = cache.latest.as_deref().and_then(|tag| notice(tag, method)) {
        output(&text);
    }
    if fresh(cache.checked_at, time) || !reserve(&path, time).unwrap_or(false) {
        return None;
    }
    Some(launch(path, time))
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}
impl Release {
    fn asset(&self, name: &str) -> Result<&Asset> {
        let mut matches = self.assets.iter().filter(|asset| asset.name == name);
        let asset = matches
            .next()
            .with_context(|| format!("Release has no {name}"))?;
        ensure!(matches.next().is_none(), "Duplicate release asset {name}");
        Ok(asset)
    }
}

struct Client {
    agent: ureq::Agent,
    base: String,
}
impl Client {
    fn new(timeout: Duration) -> Self {
        let base = format!("https://api.github.com/repos/{REPO}");
        // Production ignores this variable completely.
        #[cfg(test)]
        let base = std::env::var("SNOMED_ECL_UPDATE_BASE_URL").unwrap_or(base);
        Self {
            agent: ureq::Agent::config_builder()
                .https_only(!cfg!(test))
                .timeout_global(Some(timeout))
                .build()
                .into(),
            base,
        }
    }

    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>> {
        Ok(self
            .agent
            .get(url)
            .header("User-Agent", "snomed-ecl-engine")
            .call()?
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()?)
    }

    fn release(&self, requested: Option<&str>) -> Result<Release> {
        let endpoint = match requested {
            Some(value) => format!("tags/v{}", version(value)?),
            None => "latest".into(),
        };
        let release: Release = serde_json::from_slice(
            &self.get(&format!("{}/releases/{endpoint}", self.base), 1024 * 1024)?,
        )?;
        let parsed = version(&release.tag_name)?;
        ensure!(!release.draft, "Release is a draft");
        if let Some(requested) = requested {
            ensure!(
                parsed == version(requested)?,
                "Release version does not match --version"
            );
        } else {
            ensure!(
                !release.prerelease && parsed.pre.is_empty(),
                "Latest release is a prerelease"
            );
        }
        Ok(release)
    }

    fn download(&self, release: &Release, name: &str) -> Result<Vec<u8>> {
        let sums = self.get(
            &release.asset("SHA256SUMS")?.browser_download_url,
            1024 * 1024,
        )?;
        let expected = checksum(std::str::from_utf8(&sums)?, name)?;
        let bytes = self.get(&release.asset(name)?.browser_download_url, MAX_ARCHIVE)?;
        verify(&bytes, &expected)?;
        Ok(bytes)
    }
}

fn checksum(sums: &str, name: &str) -> Result<[u8; 32]> {
    let mut found = None;
    for line in sums.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<_> = line.split_whitespace().collect();
        ensure!(fields.len() == 2, "Malformed SHA256SUMS line");
        let hash = fields[0];
        ensure!(
            hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()),
            "Invalid SHA256SUMS digest"
        );
        if fields[1].strip_prefix('*').unwrap_or(fields[1]) == name {
            ensure!(found.is_none(), "Duplicate SHA256SUMS entry for {name}");
            let mut digest = [0; 32];
            for (i, byte) in digest.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&hash[i * 2..i * 2 + 2], 16)?;
            }
            found = Some(digest);
        }
    }
    found.with_context(|| format!("SHA256SUMS has no entry for {name}"))
}

fn verify(bytes: &[u8], expected: &[u8; 32]) -> Result<()> {
    ensure!(
        Sha256::digest(bytes).as_slice() == expected,
        "Archive SHA-256 mismatch"
    );
    Ok(())
}

fn variant() -> &'static str {
    if cfg!(feature = "unicode") {
        "unicode"
    } else if cfg!(feature = "import") {
        "default"
    } else {
        "query"
    }
}

fn asset_name(release: &Version, target: &str, build: &str) -> Result<String> {
    ensure!(
        matches!(
            target,
            "x86_64-unknown-linux-gnu"
                | "aarch64-unknown-linux-gnu"
                | "x86_64-apple-darwin"
                | "aarch64-apple-darwin"
                | "x86_64-pc-windows-msvc"
        ),
        "No release build for {target}"
    );
    ensure!(
        build != "unicode" || target.ends_with("linux-gnu"),
        "No unicode release build for {target}"
    );
    let extension = if target.ends_with("windows-msvc") {
        "zip"
    } else {
        "tar.gz"
    };
    Ok(format!(
        "snomed-ecl-engine-v{release}-{target}-{build}.{extension}"
    ))
}

/// Extract only the expected regular executable, never unpack arbitrary paths.
fn extract(bytes: &[u8], name: &str, output: &mut File) -> Result<()> {
    let stem = name
        .strip_suffix(".tar.gz")
        .or_else(|| name.strip_suffix(".zip"))
        .context("Unknown archive format")?;
    let binary = if name.ends_with(".zip") {
        "snomed-ecl-engine.exe"
    } else {
        "snomed-ecl-engine"
    };
    let expected = format!("{stem}/{binary}");
    if name.ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(io::Cursor::new(bytes))?;
        let mut found = false;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            if entry.name()? == expected {
                ensure!(!found, "Duplicate executable in archive");
                ensure!(
                    !entry.is_dir()
                        && !entry.is_symlink()
                        && entry.size() > 0
                        && entry.size() <= MAX_BINARY,
                    "Invalid executable in archive"
                );
                copy_binary(&mut entry, output)?;
                found = true;
            }
        }
        ensure!(found, "Archive has no {expected}");
    } else {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
        let mut found = false;
        for entry in archive.entries()? {
            let mut entry = entry?;
            if entry.path()?.as_ref() == Path::new(&expected) {
                ensure!(!found, "Duplicate executable in archive");
                ensure!(
                    entry.header().entry_type().is_file()
                        && entry.size() > 0
                        && entry.size() <= MAX_BINARY,
                    "Invalid executable in archive"
                );
                copy_binary(&mut entry, output)?;
                found = true;
            }
        }
        ensure!(found, "Archive has no {expected}");
    }
    Ok(())
}

fn copy_binary(reader: &mut impl Read, output: &mut File) -> Result<()> {
    let copied = io::copy(&mut reader.take(MAX_BINARY + 1), output)?;
    ensure!(
        copied > 0 && copied <= MAX_BINARY,
        "Invalid executable size"
    );
    Ok(())
}

fn replace(exe: &Path, bytes: &[u8], name: &str) -> Result<()> {
    let parent = exe.parent().context("Executable has no parent")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .context("Cannot write beside executable; use your installer's upgrade command")?;
    extract(bytes, name, temp.as_file_mut())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    temp.as_file().sync_all()?;
    #[cfg(windows)]
    {
        let old = exe.with_extension("exe.old");
        ensure!(
            !old.exists(),
            "Previous .old executable still exists; close other engine processes and retry"
        );
        fs::rename(exe, &old).context("Cannot rename running executable")?;
        if let Err(error) = temp.persist(exe) {
            fs::rename(&old, exe).context("Update failed and cannot restore .old executable")?;
            return Err(error.into());
        }
    }
    #[cfg(not(windows))]
    temp.persist(exe).context("Cannot replace executable")?;
    Ok(())
}

pub fn run(args: &[String]) -> Result<()> {
    let exe = std::env::current_exe().context("Cannot locate running executable")?;
    let method = install_method(&exe);
    run_at(args, &exe, method, &Client::new(Duration::from_secs(120)))
}

fn run_at(args: &[String], exe: &Path, method: InstallMethod, client: &Client) -> Result<()> {
    let mut args = args.to_vec();
    let check = crate::take_flag(&mut args, "--check");
    let yes = crate::take_flag(&mut args, "--yes");
    let requested = crate::take_option(&mut args, "--version")?;
    ensure!(
        args.is_empty(),
        "Usage: self-update [--check] [--version X] [--yes]"
    );
    if let Some(value) = &requested {
        version(value)?;
    }
    // Managed installs never reach the download or replacement path.
    if method != InstallMethod::Standalone && !check {
        let target = requested.as_deref().unwrap_or(env!("CARGO_PKG_VERSION"));
        let command = if method == InstallMethod::Container && requested.is_none() {
            "docker pull ghcr.io/eddiedavison92/snomed-ecl-engine:latest".into()
        } else {
            method.command(&version(target)?)
        };
        bail!("This installation is managed externally; upgrade: {command}");
    }
    let release = client.release(requested.as_deref())?;
    let target = version(&release.tag_name)?;
    let available = newer(&release.tag_name, env!("CARGO_PKG_VERSION"))?;
    if check {
        println!(
            "snomed-ecl-engine: installed {}, release {target}; {}",
            env!("CARGO_PKG_VERSION"),
            if available {
                format!("upgrade: {}", method.command(&target))
            } else {
                "up to date".into()
            }
        );
        return Ok(());
    }
    if !available {
        ensure!(
            target
                .cmp_precedence(&version(env!("CARGO_PKG_VERSION"))?)
                .is_eq(),
            "Refusing to downgrade to {target}"
        );
        println!(
            "snomed-ecl-engine {} is up to date.",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    let name = asset_name(&target, env!("SNOMED_ECL_TARGET"), variant())?;
    if !yes
        && !crate::confirm(
            &format!("Update snomed-ecl-engine to {target}?"),
            "Pass --yes to update without an interactive terminal",
        )?
    {
        return Ok(());
    }
    let bytes = client.download(&release, &name)?;
    replace(exe, &bytes, &name)?;
    println!("Updated snomed-ecl-engine to {target}.");
    Ok(())
}

#[cfg(test)]
mod tests;
