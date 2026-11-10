//! Helpers shared by the end-to-end test modules: the sandbox every test
//! runs in, the stand-in servers, and the readers for what a run wrote.

use assert_cmd::Command;
use serde_json::Value as Json;
use serde_yaml_ng::Value as Yaml;
use std::net::Ipv4Addr;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU16;
use std::sync::atomic::Ordering;
use tempfile::TempDir;

#[path = "../support/marketplace.rs"]
mod marketplace;

mod chap_core;
mod configured;
mod hub;
#[cfg(unix)]
mod run;

pub(crate) use chap_core::*;
pub(crate) use configured::*;
pub(crate) use hub::*;
#[cfg(unix)]
pub(crate) use run::*;

/// A cache directory plus the project directory the tests write into.
pub(crate) struct Sandbox {
    pub(crate) cache: TempDir,
    pub(crate) home: TempDir,
}

impl Sandbox {
    pub(crate) fn new() -> Sandbox {
        let cache = tempfile::tempdir().unwrap();
        marketplace::seed_registry_cache(cache.path());
        Sandbox {
            cache,
            home: tempfile::tempdir().unwrap(),
        }
    }

    /// The directory `chap-devops init DIR` is pointed at.
    pub(crate) fn project(&self) -> PathBuf {
        self.home.path().join("chapx")
    }

    pub(crate) fn chap(&self) -> Command {
        let mut cmd = Command::cargo_bin("chap-devops").expect("the chap-devops binary is built");
        cmd.env("CHAP_DEVOPS_CACHE_DIR", self.cache.path())
            .env("CHAP_DEVOPS_DATA_DIR", self.cache.path().join("data"))
            // A token in the developer's own shell would otherwise be sent
            // to whatever these tests point the API at, and would change what
            // the `github api` line says from one machine to the next.
            .env_remove("GITHUB_TOKEN")
            .env_remove("GH_TOKEN")
            // An `--offline` enable reads the user off a locally pulled image
            // before it falls back to the table, and what this machine has
            // pulled is not something a test may depend on.
            .env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1")
            .current_dir(self.home.path())
            .arg("--offline");
        cmd
    }

    /// `chap-devops init <project> ...`, with the arguments appended.
    pub(crate) fn init(&self, args: &[&str]) -> Command {
        let mut cmd = self.chap();
        cmd.arg("init").arg(self.project()).args(args);
        cmd
    }

    /// `chap-devops -C <project> models ...`.
    pub(crate) fn models(&self, args: &[&str]) -> Command {
        let mut cmd = self.chap();
        cmd.arg("-C").arg(self.project()).arg("models").args(args);
        cmd
    }

    /// `chap-devops -C <project> components ...`.
    pub(crate) fn components(&self, args: &[&str]) -> Command {
        let mut cmd = self.chap();
        cmd.arg("-C")
            .arg(self.project())
            .arg("components")
            .args(args);
        cmd
    }

    /// `chap-devops -C <project> open ...`.
    ///
    /// Only ever called with arguments that open nothing, or with `--no-browser`
    /// or `--json` and an empty PATH: a test that reached the opener would put a
    /// browser window on whatever machine runs the suite.
    pub(crate) fn open(&self, args: &[&str]) -> Command {
        let mut cmd = self.chap();
        cmd.arg("-C").arg(self.project()).arg("open").args(args);
        cmd
    }

    /// `chap-devops -C <project> auth ...`.
    pub(crate) fn auth(&self, args: &[&str]) -> Command {
        let mut cmd = self.chap();
        cmd.arg("-C").arg(self.project()).arg("auth").args(args);
        cmd
    }

    /// The project's `.env`, as text.
    pub(crate) fn env(&self) -> String {
        read(&self.project().join(".env"))
    }

    /// `chap-devops -C <project> ...` with the network pointed at a local
    /// stand-in rather than switched off.
    ///
    /// `chap-devops models add` resolves a repository over HTTP, so it is the one
    /// command these tests cannot run with `--offline`. Everything it would
    /// reach - GitHub, ghcr and the marketplace index - is answered by
    /// [`Hub`] on `port`, and the docker probe is turned off because a `pull`
    /// aimed at the real registry is exactly what must not happen here.
    pub(crate) fn online(&self, port: u16) -> Command {
        let base = format!("http://127.0.0.1:{port}");
        let mut cmd = Command::cargo_bin("chap-devops").expect("the chap-devops binary is built");
        cmd.env("CHAP_DEVOPS_CACHE_DIR", self.cache.path())
            .env("CHAP_DEVOPS_DATA_DIR", self.cache.path().join("data"))
            .env("CHAP_DEVOPS_NO_UPDATE_CHECK", "1")
            .env_remove("GITHUB_TOKEN")
            .env_remove("GH_TOKEN")
            .env("CHAP_DEVOPS_GITHUB_API", &base)
            .env("CHAP_DEVOPS_GITHUB_RAW", &base)
            .env("CHAP_DEVOPS_GHCR_URL", &base)
            .env("CHAP_DEVOPS_OCI_URL", &base)
            .env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1")
            .current_dir(self.home.path())
            .arg("--registry-url")
            .arg(format!("{base}/registry.yaml"))
            .arg("-C")
            .arg(self.project());
        cmd
    }

    /// `chap-devops init <project> ...` with the same network, which is how a
    /// deployment gets a chap-core pin and the compose file that goes with it
    /// without touching the real GitHub.
    pub(crate) fn online_init(&self, port: u16, args: &[&str]) -> Command {
        let base = format!("http://127.0.0.1:{port}");
        let mut cmd = Command::cargo_bin("chap-devops").expect("the chap-devops binary is built");
        cmd.env("CHAP_DEVOPS_CACHE_DIR", self.cache.path())
            .env("CHAP_DEVOPS_DATA_DIR", self.cache.path().join("data"))
            .env("CHAP_DEVOPS_NO_UPDATE_CHECK", "1")
            .env_remove("GITHUB_TOKEN")
            .env_remove("GH_TOKEN")
            .env("CHAP_DEVOPS_GITHUB_API", &base)
            .env("CHAP_DEVOPS_GITHUB_RAW", &base)
            .env("CHAP_DEVOPS_GHCR_URL", &base)
            .env("CHAP_DEVOPS_OCI_URL", &base)
            .env("CHAP_DEVOPS_NO_DOCKER_PROBE", "1")
            .current_dir(self.home.path())
            .arg("--registry-url")
            .arg(format!("{base}/registry.yaml"))
            .arg("init")
            .arg(self.project())
            .args(args);
        cmd
    }
}

pub(crate) fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The `POSTGRES_PASSWORD=` line of a generated `.env`.
pub(crate) fn password_line(env: &str) -> &str {
    env.lines()
        .find(|l| l.starts_with("POSTGRES_PASSWORD="))
        .expect("a password line")
}

/// The value of an active (uncommented) `VAR=` line of a `.env`.
pub(crate) fn env_value<'a>(env: &'a str, var: &str) -> Option<&'a str> {
    env.lines()
        .find_map(|line| line.strip_prefix(&format!("{var}=")))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn yaml(path: &Path) -> Yaml {
    serde_yaml_ng::from_str(&read(path)).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `.chap-devops/project.yaml` with `.chap-devops/models.yaml` folded in under `models`,
/// as one JSON value so assertions can index into it.
pub(crate) fn state(dir: &Path) -> Json {
    let chap_devops = dir.join(".chap-devops");
    let mut project: Json = serde_json::to_value(yaml(&chap_devops.join("project.yaml")))
        .expect("project.yaml maps to JSON");
    let models: Json = serde_json::to_value(yaml(&chap_devops.join("models.yaml")))
        .expect("models.yaml maps to JSON");
    project["models"] = models;
    project
}

/// The `--json` document one command printed, whatever it exited with:
/// `status` on a stack that is not running and `doctor` with a failing check
/// both print their report and exit non-zero.
pub(crate) fn json_of(cmd: &mut Command) -> Json {
    let out = cmd.output().expect("the command runs");
    serde_json::from_slice(&out.stdout).expect("the --json output is one document")
}

/// `chap-devops <args>` run from `cwd`, without `-C`.
pub(crate) fn chap_in(sandbox: &Sandbox, cwd: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::cargo_bin("chap-devops").expect("the chap-devops binary is built");
    cmd.env("CHAP_DEVOPS_CACHE_DIR", sandbox.cache.path())
        .env("CHAP_DEVOPS_DATA_DIR", sandbox.cache.path().join("data"))
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .current_dir(cwd)
        .arg("--offline")
        .args(args);
    cmd
}

/// A `--port-base` no other test in this binary hands out, and nothing on this
/// machine is listening on.
///
/// `--port auto` walks upwards from the base and takes the first port nothing
/// holds, and the probe behind that is a real bind. Two tests walking from the
/// same base can therefore catch each other mid-probe and be told the port is
/// taken, so a test that asserts a port it did not choose takes a base of its
/// own. `--port N` probes too, so a test naming a fixed port picks it inside
/// its own slot.
pub(crate) fn port_base() -> u16 {
    /// The first base handed out. Inside the default range, so the top of it
    /// stays 5999, and clear of the ports a developer machine tends to run
    /// (5432 for postgres, 5900 for screen sharing).
    const FIRST: u16 = 5200;
    /// Room for a test to name a fixed port or two above its base.
    const STRIDE: u16 = 10;

    static NEXT: AtomicU16 = AtomicU16::new(FIRST);
    loop {
        let base = NEXT.fetch_add(STRIDE, Ordering::Relaxed);
        assert!(base < 5990, "this test binary ran out of port bases");
        if is_free(base) {
            return base;
        }
    }
}

/// Whether `port` can be bound on both the addresses the CLI's own probe
/// tries first, which is what it takes for the CLI to call the port free.
pub(crate) fn is_free(port: u16) -> bool {
    [Ipv4Addr::UNSPECIFIED, Ipv4Addr::LOCALHOST]
        .into_iter()
        .all(|addr| std::net::TcpListener::bind((addr, port)).is_ok())
}

/// A port the kernel handed out, and that nothing is listening on.
///
/// A test that names a fixed port - 8000 for the API, 9000 for OCS - is asking
/// about whatever the developer happens to be running, not about the
/// deployment it wrote. The listener here is bound only long enough to learn
/// which port is free and is dropped before the number is handed out, so a
/// probe aimed at it finds nothing there. Ports already handed out are
/// remembered, so two calls in one test never name the same one.
pub(crate) fn free_port() -> u16 {
    static TAKEN: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a free port");
        let port = listener.local_addr().expect("a local address").port();
        drop(listener);
        let mut taken = TAKEN.lock().expect("the port list outlives its panics");
        if !taken.contains(&port) {
            taken.push(port);
            return port;
        }
    }
}

pub(crate) fn includes(dir: &Path) -> Vec<String> {
    match yaml(&dir.join("compose.marketplace.yml")).get("include") {
        Some(Yaml::Sequence(items)) => items
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// The version and image tag the frozen marketplace data's `stable` channel
/// pins for one marketplace id.
///
/// Read off `tests/fixtures/marketplace/` rather than written out here, so a
/// change there moves these assertions with it. The tests run `--offline`
/// with that data in their cache, which is what the CLI resolves against.
pub(crate) fn stable_pin(id: &str) -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("marketplace")
        .join("models")
        .join(format!("{id}.yaml"));
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let doc: Yaml = serde_yaml_ng::from_str(&body).expect("the vendored model file parses");
    let version = doc["channels"]["stable"]
        .as_str()
        .unwrap_or_else(|| panic!("{id} has no stable channel"))
        .to_string();
    let tag = doc["versions"]
        .as_sequence()
        .unwrap_or_else(|| panic!("{id} lists no versions"))
        .iter()
        .find(|v| v["version"].as_str() == Some(version.as_str()))
        .unwrap_or_else(|| panic!("{id} has no version {version}"))["image_tag"]
        .as_str()
        .unwrap_or_else(|| panic!("{id} {version} has no image tag"))
        .to_string();
    (version, tag)
}

/// The smallest stack the probe below can ask about: one service that is
/// never started. `ps` only reads, so the image is never pulled.
pub(crate) const PROBE_STACK: &str = "services:\n  probe:\n    image: alpine:3\n";

/// Whether the docker-backed tests can run here, saying why when they cannot.
///
/// `docker compose version` is not the question these tests need answered. A
/// runner can have the CLI while its daemon is unusable - not running, or in
/// Windows-containers mode - and then the call every wrapper makes,
/// `docker compose ps -a --format json`, exits 1 while `version` still
/// answers happily. That is a flaky failure, not a finding, so the probe is
/// that exact call against a stack of its own, and the tests that depend on
/// docker answering skip when it does not.
///
/// The probe runs once per test binary: the cached verdict is what every
/// caller after the first reads.
pub(crate) fn docker_ready() -> bool {
    static PROBE: OnceLock<Result<(), String>> = OnceLock::new();
    match PROBE.get_or_init(probe_docker) {
        Ok(()) => true,
        Err(reason) => {
            eprintln!("skipping: {reason}");
            false
        }
    }
}

/// Ask docker the question the tests depend on, in a directory of its own.
pub(crate) fn probe_docker() -> Result<(), String> {
    let temp = tempfile::tempdir().map_err(|e| format!("no directory to probe docker in: {e}"))?;
    let dir = temp.path().join("chap-devops-docker-probe");
    let write = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(dir.join("compose.yml"), PROBE_STACK));
    write.map_err(|e| format!("the docker probe stack could not be written: {e}"))?;

    let out = std::process::Command::new("docker")
        .args([
            "compose",
            "-f",
            "compose.yml",
            "ps",
            "-a",
            "--format",
            "json",
        ])
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("docker could not be run: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let first = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("it said nothing about why");
    Err(format!(
        "`docker compose ps -a --format json` exited with status {}: {first}",
        out.status.code().unwrap_or(-1)
    ))
}

/// The way `chap-devops` spells a deployment directory in a warning: resolved, and
/// without the `\\?\` prefix Windows' `canonicalize` puts on one.
/// `src/ports.rs` takes it off the same way, and a binary crate has nothing an
/// integration test can import, so this mirrors it. String work, so it hands
/// back the canonical path unchanged on Unix.
pub(crate) fn resolved(path: &Path) -> String {
    let real = std::fs::canonicalize(path).expect("the directory exists");
    let text = real.display().to_string();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

/// The one file in `dir` whose name looks like a backup archive.
pub(crate) fn only_archive(dir: &Path) -> PathBuf {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".tar.gz"))
        .collect();
    found.sort();
    assert_eq!(found.len(), 1, "expected one archive in {}", dir.display());
    found.pop().unwrap()
}

/// A server that answers every request with the same 200, on a port of its
/// own. The thread lives as long as the test process.
pub(crate) fn server(content_type: &'static str, body: &'static str) -> u16 {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a free port");
    let port = listener.local_addr().expect("a local address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // The request has to be read before the answer, or the client
            // sees a reset instead of the response.
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

/// A server that answers `ok_path` with a 200 and every other path with a 404,
/// on a port of its own.
///
/// The two shapes a DHIS2 container takes from outside. One where the Spring
/// context came up, and `/api/ping` answers - the route DHIS2 serves without
/// credentials, and the one the container's own healthcheck uses. And one where
/// it did not: Tomcat keeps serving, and every `/api/*` request 404s.
pub(crate) fn routed(ok_path: Option<&'static str>, body: &'static str) -> u16 {
    use std::io::{BufRead, BufReader, Write};

    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a free port");
    let port = listener.local_addr().expect("a local address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = String::new();
            // The request has to be read before the answer, or the client sees
            // a reset instead of the response.
            if BufReader::new(&stream).read_line(&mut request).is_err() {
                continue;
            }
            let path = request.split_whitespace().nth(1).unwrap_or_default();
            let ok = ok_path == Some(path);
            let (code, reason) = if ok { (200, "OK") } else { (404, "Not Found") };
            let response = format!(
                "HTTP/1.1 {code} {reason}\r\nContent-Type: text/plain\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

/// The active `SERVICEKIT_REGISTRATION_KEY:` line of a model overlay, which is
/// there only when the deployment has a registration key.
pub(crate) fn overlay_sends_the_key(dir: &Path) -> bool {
    read(&dir.join("compose.chapkit-ewars-model.yml"))
        .contains("\n      SERVICEKIT_REGISTRATION_KEY: ${SERVICEKIT_REGISTRATION_KEY:-}\n")
}

/// A `chap-devops` run that needs no project: outside one, with its own cache and no
/// update check, so nothing here can reach the network.
pub(crate) fn bare() -> (TempDir, Command) {
    let cache = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("chap-devops").expect("the chap-devops binary is built");
    cmd.env("CHAP_DEVOPS_CACHE_DIR", cache.path())
        .env("CHAP_DEVOPS_DATA_DIR", cache.path().join("data"))
        .env("CHAP_DEVOPS_NO_UPDATE_CHECK", "1")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .current_dir(cache.path());
    (cache, cmd)
}

/// The one check of a `doctor --json` report with this id.
pub(crate) fn doctor_check<'a>(report: &'a Json, id: &str) -> &'a Json {
    report["checks"]
        .as_array()
        .unwrap_or_else(|| panic!("`checks` is not an array in {report}"))
        .iter()
        .find(|check| check["id"] == id)
        .unwrap_or_else(|| panic!("no check `{id}` in {report}"))
}

/// The status word of that check.
pub(crate) fn doctor_status<'a>(report: &'a Json, id: &str) -> &'a str {
    doctor_check(report, id)["status"]
        .as_str()
        .unwrap_or_else(|| panic!("`{id}` has no status"))
}

/// A named volume a test created, removed again however the test ends.
pub(crate) struct Volume(String);

impl Volume {
    /// Create it, as compose would when it first starts the service.
    pub(crate) fn create(name: &str) -> Volume {
        Volume::create_with(name, &[])
    }

    /// Create it with the compose project label compose gives it.
    pub(crate) fn create_for_project(name: &str, project: &str) -> Volume {
        let label = format!("com.docker.compose.project={project}");
        Volume::create_with(name, &["--label", &label])
    }

    /// Create it with both labels compose gives a volume it declares.
    /// Compose 5 removes on `down -v` only a volume with these labels; it
    /// skips any other as `Volume was not created by Docker Compose`.
    pub(crate) fn create_by_compose(name: &str, project: &str, volume: &str) -> Volume {
        let project = format!("com.docker.compose.project={project}");
        let volume = format!("com.docker.compose.volume={volume}");
        Volume::create_with(name, &["--label", &project, "--label", &volume])
    }

    fn create_with(name: &str, flags: &[&str]) -> Volume {
        let out = std::process::Command::new("docker")
            .args(["volume", "create"])
            .args(flags)
            .arg(name)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("docker volume create runs");
        assert!(
            out.status.success(),
            "docker volume create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Volume(name.to_string())
    }
}

impl Drop for Volume {
    fn drop(&mut self) {
        let _ = std::process::Command::new("docker")
            .args(["volume", "rm", "-f", &self.0])
            .stdin(std::process::Stdio::null())
            .output();
    }
}
