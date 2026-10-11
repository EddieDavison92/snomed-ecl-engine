use super::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, Transport,
};

#[test]
fn semantic_version_order() {
    for (candidate, current, expected) in [
        ("v0.10.0", "0.9.9", true),
        ("1.0.0", "0.99.99", true),
        ("1.0.0", "1.0.0-rc.1", true),
        ("1.0.0-rc.10", "1.0.0-rc.2", true),
        ("1.0.0-alpha", "1.0.0", false),
        ("0.2.0", "0.3.0", false),
        ("1.0.0+new", "1.0.0+old", false),
        ("v1.0.0", "1.0.0", false),
    ] {
        assert_eq!(newer(candidate, current).unwrap(), expected);
    }
    for bad in ["latest", "1.2", "v01.2.3", "1.2.3\n", "../../x"] {
        assert!(version(bad).is_err());
    }
}

#[test]
fn install_layouts_and_commands() {
    let release = version("v1.2.3").unwrap();
    for (path, home, marker, container, method, command) in [
        ("/usr/lib/node_modules/snomed-ecl-engine-linux-x64/bin/snomed-ecl-engine", None, None, false, InstallMethod::Npm, "npm i -g snomed-ecl-engine"),
        ("C:\\Users\\me\\AppData\\npm\\node_modules\\snomed-ecl-engine-win32-x64\\bin\\snomed-ecl-engine.exe", None, None, false, InstallMethod::Npm, "npm i -g snomed-ecl-engine"),
        ("/home/me/.cargo/bin/snomed-ecl-engine", None, None, false, InstallMethod::Cargo, "cargo install snomed-ecl-engine"),
        ("/custom/cargo/bin/snomed-ecl-engine", Some("/custom/cargo"), None, false, InstallMethod::Cargo, "cargo install snomed-ecl-engine"),
        ("/opt/homebrew/Cellar/snomed-ecl-engine/0.2.1/bin/snomed-ecl-engine", None, None, false, InstallMethod::Homebrew, "brew upgrade snomed-ecl-engine"),
        ("/home/linuxbrew/.linuxbrew/opt/snomed-ecl-engine/bin/snomed-ecl-engine", None, None, false, InstallMethod::Homebrew, "brew upgrade snomed-ecl-engine"),
        ("/usr/local/bin/snomed-ecl-engine", None, Some("container"), false, InstallMethod::Container, "docker pull ghcr.io/eddiedavison92/snomed-ecl-engine:1.2.3"),
        ("/usr/local/bin/snomed-ecl-engine", None, None, true, InstallMethod::Container, "docker pull ghcr.io/eddiedavison92/snomed-ecl-engine:1.2.3"),
        ("/tmp/tool", None, Some("npm"), true, InstallMethod::Npm, "npm i -g snomed-ecl-engine"),
        ("/home/me/.local/bin/snomed-ecl-engine", None, None, false, InstallMethod::Standalone, "snomed-ecl-engine self-update"),
    ] {
        let detected = detect_install(Path::new(path), home.map(Path::new), marker, container);
        assert_eq!(detected, method, "{path}");
        assert_eq!(detected.command(&release), command);
    }
}

#[test]
fn suppression_rules() {
    let args = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(!suppressed(&args(&["--version"]), true, |_| None));
    assert!(suppressed(&[], false, |_| None));
    for flag in ["--json", "--plain", "self-update"] {
        assert!(suppressed(&args(&[flag]), true, |_| None));
    }
    assert!(suppressed(&[], true, |key| (key
        == "SNOMED_ECL_NO_UPDATE_CHECK")
        .then(|| "1".into())));
    assert!(!suppressed(&[], true, |key| (key
        == "SNOMED_ECL_NO_UPDATE_CHECK")
        .then(|| "0".into())));
    for ci in CI_ENV {
        assert!(
            suppressed(&[], true, |key| (key == *ci).then(String::new)),
            "{ci}"
        );
    }
}

#[test]
fn cache_ttl_and_exclusive_reservation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    let time = DAY * 100;
    assert!(fresh(time, time));
    assert!(fresh(time, time + DAY - 1));
    assert!(!fresh(time, time + DAY));
    assert!(fresh(time, time - 1));
    assert!(reserve(&path, time).unwrap());
    assert!(!reserve(&path, time + 1).unwrap());
    assert!(reserve(&path, time + DAY).unwrap());
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))
        .unwrap();
    lock.lock().unwrap();
    assert!(!reserve(&path, time + 2 * DAY).unwrap());
    drop(lock);
    assert!(reserve(&path, time + 2 * DAY).unwrap());
    fs::write(&path, "malformed JSON").unwrap();
    assert!(reserve(&path, time + 2 * DAY).unwrap());
}

#[test]
fn checksums_fail_closed() {
    let digest = format!("{:x}", Sha256::digest(b"archive"));
    for separator in ["  ", " *", "\t"] {
        let sum = checksum(
            &format!("{digest}{separator}asset.tar.gz\r\n"),
            "asset.tar.gz",
        )
        .unwrap();
        verify(b"archive", &sum).unwrap();
        assert!(verify(b"tampered", &sum).is_err());
    }
    for sums in [
        String::new(),
        format!("{digest}  other"),
        "abc  asset.tar.gz".into(),
        format!("{}  asset.tar.gz", "g".repeat(64)),
        format!("{digest}  asset.tar.gz\n{digest}  asset.tar.gz"),
        format!("{digest}  asset.tar.gz extra"),
    ] {
        assert!(checksum(&sums, "asset.tar.gz").is_err(), "{sums}");
    }
}

#[test]
fn platform_assets_match_release_layout() {
    let v = version("1.2.3").unwrap();
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ] {
        let ext = if target.ends_with("msvc") {
            "zip"
        } else {
            "tar.gz"
        };
        for build in ["default", "query"] {
            assert_eq!(
                asset_name(&v, target, build).unwrap(),
                format!("snomed-ecl-engine-v1.2.3-{target}-{build}.{ext}")
            );
        }
    }
    assert!(asset_name(&v, "x86_64-unknown-linux-musl", "default").is_err());
    assert!(asset_name(&v, "aarch64-apple-darwin", "unicode").is_err());
    assert!(asset_name(&v, "aarch64-unknown-linux-gnu", "unicode").is_ok());
}

fn tar_fixture(name: &str, binary: &[u8], symlink: bool) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_mode(0o755);
    header.set_size(if symlink { 0 } else { binary.len() as u64 });
    if symlink {
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_link_name("/etc/passwd").unwrap();
    }
    header.set_cksum();
    archive
        .append_data(
            &mut header,
            format!("{}/snomed-ecl-engine", name.trim_end_matches(".tar.gz")),
            if symlink { &[][..] } else { binary },
        )
        .unwrap();
    archive.into_inner().unwrap().finish().unwrap()
}

#[test]
fn replacement_and_safe_extraction() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("snomed-ecl-engine.exe");
    fs::write(&exe, b"old binary").unwrap();
    let name = "snomed-ecl-engine-v99.1.0-x86_64-unknown-linux-gnu-default.tar.gz";
    for bytes in [
        b"invalid gzip".to_vec(),
        tar_fixture(name, b"", false),
        tar_fixture(name, b"", true),
        tar_fixture("other.tar.gz", b"new", false),
    ] {
        assert!(replace(&exe, &bytes, name).is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"old binary");
    }
    replace(&exe, &tar_fixture(name, b"new binary", false), name).unwrap();
    assert_eq!(fs::read(&exe).unwrap(), b"new binary");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&exe).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    #[cfg(windows)]
    assert_eq!(
        fs::read(exe.with_extension("exe.old")).unwrap(),
        b"old binary"
    );
}

#[test]
fn windows_zip_extraction() {
    let name = "snomed-ecl-engine-v99.1.0-x86_64-pc-windows-msvc-default.zip";
    let mut archive = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    archive
        .start_file(
            format!("{}/snomed-ecl-engine.exe", name.trim_end_matches(".zip")),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive.write_all(b"windows binary").unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let mut output = tempfile::tempfile().unwrap();
    extract(&bytes, name, &mut output).unwrap();
    use std::io::{Seek, SeekFrom};
    output.seek(SeekFrom::Start(0)).unwrap();
    let mut actual = Vec::new();
    output.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, b"windows binary");
}

type Routes = HashMap<String, (u16, Vec<u8>)>;

/// A local fixture HTTP server behind ureq's mock transport. This exercises
/// real HTTP encoding/decoding without needing a socket or external DNS.
#[derive(Clone, Debug)]
struct Server {
    base: String,
    routes: Arc<Mutex<Routes>>,
    requests: Arc<Mutex<Vec<String>>>,
    delay: Duration,
}
impl Server {
    fn new(delay: Duration) -> Self {
        Self {
            base: "http://127.0.0.1:12345".into(),
            routes: Arc::default(),
            requests: Arc::default(),
            delay,
        }
    }
    fn set(&self, path: &str, status: u16, body: impl Into<Vec<u8>>) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.into(), (status, body.into()));
    }
    fn client(&self, timeout: Duration) -> Client {
        let config = ureq::Agent::config_builder()
            .proxy(None)
            .https_only(false)
            .timeout_global(Some(timeout))
            .build();
        Client {
            agent: ureq::Agent::with_parts(
                config,
                self.clone(),
                ureq::unversioned::resolver::DefaultResolver::default(),
            ),
            base: self.base.clone(),
        }
    }
    fn release(&self, name: &str) -> Vec<u8> {
        include_str!("test-release.json")
            .replace("BASE", &self.base)
            .replace("ASSET", name)
            .into_bytes()
    }
}
impl Connector for Server {
    type Out = FixtureTransport;
    fn connect(
        &self,
        _: &ConnectionDetails,
        _: Option<()>,
    ) -> std::result::Result<Option<Self::Out>, ureq::Error> {
        Ok(Some(FixtureTransport {
            server: self.clone(),
            buffers: LazyBuffers::new(4096, 4096),
            request: Vec::new(),
            reply: io::Cursor::new(Vec::new()),
            replied: false,
        }))
    }
}
#[derive(Debug)]
struct FixtureTransport {
    server: Server,
    buffers: LazyBuffers,
    request: Vec<u8>,
    reply: io::Cursor<Vec<u8>>,
    replied: bool,
}
impl Transport for FixtureTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }
    fn transmit_output(
        &mut self,
        amount: usize,
        _: NextTimeout,
    ) -> std::result::Result<(), ureq::Error> {
        self.request
            .extend_from_slice(&self.buffers.output()[..amount]);
        Ok(())
    }
    fn await_input(&mut self, timeout: NextTimeout) -> std::result::Result<bool, ureq::Error> {
        if !self.replied {
            let request = std::str::from_utf8(&self.request).unwrap();
            assert!(request.ends_with("\r\n\r\n"));
            let path = request.split_whitespace().nth(1).unwrap().to_owned();
            self.server.requests.lock().unwrap().push(path.clone());
            if self.server.delay > *timeout.after {
                std::thread::sleep(*timeout.after);
                return Err(ureq::Error::Timeout(timeout.reason));
            }
            std::thread::sleep(self.server.delay);
            let (status, body) = self
                .server
                .routes
                .lock()
                .unwrap()
                .get(&path)
                .cloned()
                .unwrap_or((404, Vec::new()));
            let mut reply = format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            reply.extend(body);
            self.reply = io::Cursor::new(reply);
            self.replied = true;
        }
        let read = self.reply.read(self.buffers.input_append_buf())?;
        self.buffers.input_appended(read);
        Ok(read > 0)
    }
    fn is_open(&mut self) -> bool {
        !self.replied
    }
}

fn start_check(
    path: PathBuf,
    method: InstallMethod,
    time: u64,
    client: Client,
    output: impl Fn(String),
) -> Option<std::thread::JoinHandle<()>> {
    schedule_check(
        path,
        method,
        time,
        |text| output(text.to_owned()),
        |path, time| std::thread::spawn(move || refresh_cache(&path, time, &client)),
    )
}

#[test]
fn local_http_release_download_and_failures() {
    let server = Server::new(Duration::ZERO);
    let name = "snomed-ecl-engine-v99.1.0-x86_64-unknown-linux-gnu-default.tar.gz";
    let archive = tar_fixture(name, b"verified binary", false);
    let sums = format!("{:x}  {name}\n", Sha256::digest(&archive));
    server.set("/releases/latest", 200, server.release(name));
    server.set("/releases/tags/v99.1.0", 200, server.release(name));
    server.set("/SHA256SUMS", 200, sums.clone());
    server.set("/archive", 200, archive.clone());
    let client = server.client(Duration::from_secs(1));
    let release = client.release(None).unwrap();
    assert!(newer(&release.tag_name, env!("CARGO_PKG_VERSION")).unwrap());
    assert_eq!(client.release(Some("v99.1.0")).unwrap().tag_name, "v99.1.0");
    let bytes = client.download(&release, name).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("snomed-ecl-engine.exe");
    fs::write(&exe, b"original").unwrap();
    replace(&exe, &bytes, name).unwrap();
    assert_eq!(fs::read(exe).unwrap(), b"verified binary");
    server.set("/archive", 200, b"corrupt".to_vec());
    assert!(client
        .download(&release, name)
        .unwrap_err()
        .to_string()
        .contains("mismatch"));
    server.set(
        "/SHA256SUMS",
        200,
        format!("{:x}  unrelated\n", Sha256::digest(&archive)),
    );
    assert!(client
        .download(&release, name)
        .unwrap_err()
        .to_string()
        .contains("no entry"));
    server.set("/SHA256SUMS", 404, Vec::new());
    assert!(client.download(&release, name).is_err());
    let missing = Release {
        assets: Vec::new(),
        ..release
    };
    assert!(client.download(&missing, name).is_err());
    server.set("/releases/latest", 200, b"{not json}".to_vec());
    assert!(client.release(None).is_err());
    server.set("/releases/latest", 503, Vec::new());
    assert!(client.release(None).is_err());
    assert!(client.release(Some("../../bad")).is_err());
}

#[test]
fn command_check_and_managed_install_refusal() {
    let server = Server::new(Duration::ZERO);
    server.set("/releases/latest", 200, server.release("unused"));
    let client = server.client(Duration::from_secs(1));
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("engine");
    fs::write(&exe, b"original").unwrap();
    let args = |values: &[&str]| values.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    run_at(
        &args(&["--check"]),
        &exe,
        InstallMethod::Standalone,
        &client,
    )
    .unwrap();
    assert_eq!(
        server.requests.lock().unwrap().as_slice(),
        &["/releases/latest"]
    );
    assert_eq!(fs::read(&exe).unwrap(), b"original");
    server.requests.lock().unwrap().clear();
    for method in [
        InstallMethod::Npm,
        InstallMethod::Cargo,
        InstallMethod::Homebrew,
        InstallMethod::Container,
    ] {
        let error = run_at(&args(&["--yes"]), &exe, method, &client)
            .unwrap_err()
            .to_string();
        let command = if method == InstallMethod::Container {
            "docker pull ghcr.io/eddiedavison92/snomed-ecl-engine:latest".into()
        } else {
            method.command(&version("99.1.0").unwrap())
        };
        assert!(error.contains(&command));
        assert_eq!(fs::read(&exe).unwrap(), b"original");
    }
    assert!(server.requests.lock().unwrap().is_empty());
    for values in [
        &["--version"][..],
        &["--unknown"],
        &["--version", "../bad"],
        &["--version", "1.2.3", "--version", "1.2.4"],
    ] {
        assert!(run_at(&args(values), &exe, InstallMethod::Standalone, &client).is_err());
    }
    assert!(server.requests.lock().unwrap().is_empty());
}

#[test]
fn explicit_release_updates_and_downgrades_are_refused() {
    let server = Server::new(Duration::ZERO);
    let client = server.client(Duration::from_secs(1));
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("engine.exe");
    fs::write(&exe, b"original").unwrap();
    let name = asset_name(
        &version("99.1.0").unwrap(),
        env!("SNOMED_ECL_TARGET"),
        variant(),
    )
    .unwrap();
    let archive = if name.ends_with(".zip") {
        let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        zip.start_file(
            format!("{}/snomed-ecl-engine.exe", name.trim_end_matches(".zip")),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"replacement").unwrap();
        zip.finish().unwrap().into_inner()
    } else {
        tar_fixture(&name, b"replacement", false)
    };
    server.set("/releases/tags/v99.1.0", 200, server.release(&name));
    server.set(
        "/SHA256SUMS",
        200,
        format!("{:x}  {name}\n", Sha256::digest(&archive)),
    );
    server.set("/archive", 200, archive);
    run_at(
        &["--version".into(), "v99.1.0".into(), "--yes".into()],
        &exe,
        InstallMethod::Standalone,
        &client,
    )
    .unwrap();
    assert_eq!(fs::read(&exe).unwrap(), b"replacement");
    server.requests.lock().unwrap().clear();
    server.set(
        "/releases/tags/v0.0.1",
        200,
        serde_json::to_vec(&serde_json::json!({"tag_name":"v0.0.1"})).unwrap(),
    );
    let error = run_at(
        &["--version".into(), "0.0.1".into(), "--yes".into()],
        &exe,
        InstallMethod::Standalone,
        &client,
    )
    .unwrap_err();
    assert!(error.to_string().contains("downgrade"));
    assert_eq!(
        server.requests.lock().unwrap().as_slice(),
        &["/releases/tags/v0.0.1"]
    );
    assert_eq!(fs::read(&exe).unwrap(), b"replacement");
}

#[test]
fn background_check_is_cached_and_prints_once() {
    let server = Server::new(Duration::ZERO);
    server.set("/releases/latest", 200, server.release("unused"));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    let messages = Arc::new(Mutex::new(Vec::new()));
    let time = now();
    let output = messages.clone();
    start_check(
        path.clone(),
        InstallMethod::Npm,
        time,
        server.client(Duration::from_secs(1)),
        move |text| output.lock().unwrap().push(text),
    )
    .unwrap()
    .join()
    .unwrap();
    assert_eq!(read_cache(&path).latest.as_deref(), Some("v99.1.0"));
    assert!(messages.lock().unwrap().is_empty()); // first lookup is silent

    let output = messages.clone();
    assert!(start_check(
        path.clone(),
        InstallMethod::Npm,
        time + DAY - 1,
        server.client(Duration::from_secs(1)),
        move |text| output.lock().unwrap().push(text)
    )
    .is_none());
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    let output = messages.clone();
    start_check(
        path,
        InstallMethod::Npm,
        time + DAY,
        server.client(Duration::from_secs(1)),
        move |text| output.lock().unwrap().push(text),
    )
    .unwrap()
    .join()
    .unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    assert_eq!(
        messages.lock().unwrap().as_slice(),
        &[
            "snomed-ecl-engine 99.1.0 is available; upgrade: npm i -g snomed-ecl-engine",
            "snomed-ecl-engine 99.1.0 is available; upgrade: npm i -g snomed-ecl-engine",
        ]
    ); // one notice per cached run, including stale cache
}

#[test]
fn slow_or_bad_checks_do_not_block_or_report_errors() {
    for (delay, body) in [
        (Duration::from_millis(200), b"{}".to_vec()),
        (Duration::ZERO, b"not JSON".to_vec()),
    ] {
        let server = Server::new(delay);
        server.set("/releases/latest", 200, body);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        let time = std::time::Instant::now();
        let worker = start_check(
            path.clone(),
            InstallMethod::Standalone,
            now(),
            server.client(Duration::from_millis(50)),
            |_| panic!("failed check emitted a notice"),
        )
        .unwrap();
        assert!(time.elapsed() < Duration::from_millis(100));
        worker.join().unwrap();
        assert!(read_cache(&path).latest.is_none());
        assert!(!reserve(&path, now()).unwrap());
    }
}

#[test]
fn latest_rejects_prereleases_and_drafts() {
    let server = Server::new(Duration::ZERO);
    let client = server.client(Duration::from_secs(1));
    for (tag, draft, pre) in [
        ("v99.1.0-rc.1", false, false),
        ("v99.1.0", true, false),
        ("v99.1.0", false, true),
        ("not-a-version", false, false),
    ] {
        server.set(
            "/releases/latest",
            200,
            serde_json::to_vec(
                &serde_json::json!({ "tag_name":tag, "draft":draft, "prerelease":pre }),
            )
            .unwrap(),
        );
        assert!(client.release(None).is_err());
    }
    assert!(notice("v0.1.0", InstallMethod::Standalone).is_none());
    assert!(notice("malformed", InstallMethod::Standalone).is_none());
}
#[test]
fn cargo_custom_root_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("bin/snomed-ecl-engine");
    fs::create_dir(exe.parent().unwrap()).unwrap();
    fs::write(&exe, b"binary").unwrap();
    fs::write(dir.path().join(".crates.toml"), "synthetic cargo receipt").unwrap();
    assert_eq!(install_method(&exe), InstallMethod::Cargo);
}
