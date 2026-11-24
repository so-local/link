//! Folder-backed clouds for environment tests. Start at
//! [`World::new`]: each world owns a home, project, programs and result
//! bucket. [`standins`] holds the fake commands; the rest of this module
//! wires them together and exposes observations for assertions.
//!
//! Real workload programs run against these stand-ins.
//! This fixture does not prove kernel confinement or provider behavior.

mod standins;

pub use standins::{FILE, JOBCLOUD};

pub use std::io::{Read, Write};

pub use std::path::{Path, PathBuf};

pub use std::process::{Command, Output, Stdio};

pub use std::sync::{Arc, Mutex};

pub use link_test_kit::TempDir;

/// A user namespace cannot supply a host's box identity, even for a
/// folder-backed cloud. The production kind reader deliberately refuses
/// that assertion; these tests must run on a host with a whole UID map.
pub fn box_identity() -> bool {
    #[cfg(target_os = "linux")]
    if !std::fs::read_to_string("/proc/self/uid_map")
        .is_ok_and(|map| link_machine::machine::whole_uid_map(&map))
    {
        return link_test_kit::gate::lacks(
            "host identity",
            "a box fixture needs a whole UID map",
            Some(link_test_kit::gate::SANDBOX),
        );
    }
    true
}

/// The released workspace's programs (`lnk-box` and the modules beside
/// it), which these modules find on PATH as an installed Link has them.
pub fn released() -> PathBuf {
    link_test_kit::programs()
}

pub fn env_bin() -> String {
    link_test_kit::programs()
        .join("lnk-env")
        .display()
        .to_string()
}

pub fn program(path: &Path, body: &str) {
    link_test_kit::write_program(path, format!("#!/bin/sh\n{body}\n")).unwrap();
}

pub struct World {
    pub dir: TempDir,
    pub tmp: TempDir,
    pub bin: PathBuf,
    pub env: PathBuf,
    pub _results: ResultServer,
    programs: PathBuf,
}

impl World {
    pub fn new(test: &str) -> World {
        Self::with_programs(test, released())
    }

    /// The same fixture, using a caller-owned set of built programs.
    pub fn with_programs(test: &str, programs: PathBuf) -> World {
        let dir = TempDir::new(&format!("env-{test}"));
        // Job workers append a private directory and control socket to TMPDIR.
        // Keep that path short without moving trusted fixture state into /tmp.
        let tmp = TempDir::short("env");
        let bin = dir.join("bin");
        let project = dir.join("project");
        for folder in [&bin, &project, &dir.join("home"), &dir.join("boxes.d")] {
            std::fs::create_dir_all(folder).unwrap();
        }
        for spec in ["deps.toml", "clone.toml", "db.toml", "run.toml"] {
            std::fs::write(
                project.join(spec),
                "schema = 1\nname = \"s\"\nversion = 1\n",
            )
            .unwrap();
        }
        std::fs::write(project.join("requirements.txt"), "flask\n").unwrap();
        std::fs::write(project.join("README.md"), "the app\n").unwrap();
        std::fs::write(project.join("env.toml"), FILE).unwrap();
        // What a box's first boot writes, saying it's a box: each stand-in
        // box reads it (`LNK_TEST_MACHINE_KIND`), this computer none.
        stand_in_box(&dir.join("box"));
        let root = dir.display().to_string();
        let lnk_env = programs.join("lnk-env").display().to_string();
        // This computer's lnk: boxes are folders in boxes.d, each with a
        // home; an image is a copy of a box's home, by its hash.
        program(&bin.join("lnk"), &standins::controller(&root, &lnk_env));
        // A box's lnk: `sandbox run` runs the command with what
        // `--with-fd` gives (a stand-in for the sandbox, which its own
        // tests cover), keeping what it was given beside the box's home,
        // each secret's value blanked: the real sandbox hands it to its
        // proxy by pipe, never writing it, so the whole box is scanned.
        program(&bin.join("boxlnk"), standins::BOX);
        program(&bin.join("sudo"), standins::SUDO);
        program(&bin.join("git"), standins::GIT);
        // Three accounts: aws, whose adapter runs no job itself (so a box
        // is made for each), jobcloud, a cloud that runs them, and store,
        // which keeps their results.
        std::fs::create_dir_all(dir.join("clouds.d")).unwrap();
        for (name, kind) in [("aws", "aws"), ("jobcloud", "jobcloud"), ("store", "store")] {
            std::fs::write(
                dir.join(format!("clouds.d/{name}.json")),
                format!(r#"{{"name": "{name}", "kind": "{kind}", "place": "here"}}"#),
            )
            .unwrap();
        }
        program(&bin.join("lnk-aws"), standins::AWS);
        // store, an account whose buckets are folders of bucket/, each
        // object a file there: where a job's results go.
        program(&bin.join("lnk-store"), &standins::store(&root));
        link_test_kit::write_program(
            &bin.join("lnk-jobcloud"),
            format!("#!/usr/bin/env python3\nROOT = {root:?}\n{JOBCLOUD}"),
        )
        .unwrap();
        let results = ResultServer::new(&dir);
        std::fs::write(dir.join("results-url"), &results.url).unwrap();
        link_test_kit::write_program(&bin.join("systemd-run"), standins::SYSTEMD_RUN).unwrap();
        World {
            _results: results,
            programs,
            env: project.join("env.toml"),
            dir,
            tmp,
            bin,
        }
    }

    /// A job file, `project/ci.toml`, of the environment and one stage,
    /// `command`, running `run` in `run.toml` (as root when `root`): its
    /// path.
    pub fn job(&self, run: &str, root: bool) -> String {
        let path = self.dir.join("project/ci.toml");
        let place = match root {
            true => "user = \"root\"",
            false => "sandbox = \"run.toml\"",
        };
        let text = format!(
            "schema = 1\nname = \"ci\"\nversion = 1\nenv = \"env.toml\"\n\n[[stage]]\nname = \
             \"command\"\nrun = {}\n{place}\n",
            toml::Value::String(run.into())
        );
        std::fs::write(&path, text).unwrap();
        std::fs::canonicalize(&path).unwrap().display().to_string()
    }

    /// `lnk-env <args>`; a job of `-- <command>` given no `--sandbox` or
    /// `--user` runs in the project's `run.toml`.
    pub fn command(&self, args: &[&str]) -> Command {
        let mut args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let before = args.iter().position(|a| a == "--").unwrap_or(args.len());
        let placed = args[..before]
            .iter()
            .any(|a| a == "--sandbox" || a == "--user" || a == "--spec");
        if args.starts_with(&["job".into(), "run".into()]) && !placed {
            let run = self.dir.join("project/run.toml").display().to_string();
            args.splice(2..2, ["--sandbox".to_string(), run]);
        }
        // Keep the controller's recorder and isolate the child just as every
        // other fixture launched through the testing kit does.
        let mut command = link_test_kit::env::command(
            if args.first().is_some_and(|s| s == "job") {
                self.programs.join("lnk-job")
            } else {
                self.programs.join("lnk-env")
            },
            &self.dir.join("home"),
        );
        // Neither lnk-env nor lnk-job reads its own namespace: `env check`
        // is `lnk-env check`, and `job run` is `lnk-job run`.
        let given = match args.first().is_some_and(|s| s == "env" || s == "job") {
            true => &args[1..],
            false => &args[..],
        };
        command
            .args(given)
            .env("HOME", self.dir.join("home"))
            .env("TMPDIR", &self.tmp)
            // The fake systemd launcher accepts any account name.
            .env("USER", "test-user")
            .env_remove("XDG_CONFIG_HOME")
            .env("LNK_SERVICES", "off")
            .envs(link_test_kit::keychain::isolated(&self.dir.join("home")))
            .env("LNK_EXE", self.bin.join("lnk"))
            .env("LNK_SUDO", self.bin.join("sudo"))
            .env("LNK_TEST_ENV_BIN", self.programs.join("lnk-env"))
            .env("LNK_TEST_JOB_BIN", self.programs.join("lnk-job"))
            .env("PATH", self.path());
        command
    }

    /// The PATH the programs it runs are given: its own stand-ins first,
    /// then the released programs, then the system's.
    pub fn path(&self) -> String {
        format!(
            "{}:{}:/usr/bin:/bin",
            self.bin.display(),
            self.programs.display()
        )
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.command(args).stdin(Stdio::null()).output().unwrap()
    }

    pub fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.dir.join(path)).unwrap_or_default()
    }

    /// Detached jobs publish their final record before powering off.
    /// Keep the fixture alive until its stand-in actually removes the box.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    pub fn wait_removed(&self, name: &str) {
        let started = std::time::Instant::now();
        while !self.read("removed").lines().any(|removed| removed == name) {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(10),
                "box {name} was not removed"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    pub fn env_arg(&self) -> String {
        self.env.display().to_string()
    }

    /// Every file under `folder`, with what's in it.
    pub fn everything_in(&self, folder: &str) -> String {
        pub fn walk(dir: &Path, out: &mut String) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push_str(&path.display().to_string());
                    out.push('\n');
                    out.push_str(&String::from_utf8_lossy(
                        &std::fs::read(&path).unwrap_or_default(),
                    ));
                    out.push('\n');
                }
            }
        }
        let mut out = String::new();
        walk(&self.dir.join(folder), &mut out);
        out
    }
}

/// `folder/kind.toml` as `lnk box create` writes a box's
/// `/etc/lnk/kind.toml`, written by no one else: its path.
pub fn stand_in_box(folder: &Path) -> PathBuf {
    pub use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(folder).unwrap();
    let path = folder.join("kind.toml");
    std::fs::write(
        &path,
        "kind = \"box\"\nos = \"ubuntu-24.04\"\narch = \"x86_64\"\n",
    )
    .unwrap();
    for (path, mode) in [(folder, 0o755), (path.as_path(), 0o644)] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    path
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The hash `lnk env check` says for the world's file.
pub fn hash_of(world: &World) -> String {
    let out = world.run(&["env", "check", "--spec", &world.env_arg(), "--json"]);
    let checked: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    checked["hash"].as_str().unwrap().to_string()
}

/// What a hook server was posted.
pub fn hooks() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://127.0.0.1:{}/hook",
        listener.local_addr().unwrap().port()
    );
    let posted = Arc::new(Mutex::new(Vec::new()));
    let kept = posted.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if body.len() >= length {
                        kept.lock().unwrap().push(body.to_string());
                        break;
                    }
                }
            }
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    (url, posted)
}

/// No box was made, copied to, run on or saved through `lnk box`: what
/// this computer knows of its boxes is all it asked.
pub fn made_no_box(world: &World) {
    let lnk = world.read("lnk.log");
    for line in lnk.lines().filter(|l| l.starts_with("lnk box")) {
        assert!(line.starts_with("lnk box list --here"), "{lnk}");
    }
}

/// Each request `lnk-jobcloud` was sent, as (command, request).
pub fn jobcloud_asked(world: &World) -> Vec<(String, serde_json::Value)> {
    world
        .read("jobcloud.log")
        .lines()
        .map(|l| {
            let (command, request) = l.split_once(' ').unwrap();
            (command.to_string(), serde_json::from_str(request).unwrap())
        })
        .collect()
}

/// Conditional object PUTs: the test bucket accepts each object exactly once.
pub struct ResultServer {
    pub url: String,
    pub stop: Arc<std::sync::atomic::AtomicBool>,
    pub thread: Option<std::thread::JoinHandle<()>>,
}

impl ResultServer {
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn new(root: &Path) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ending = stop.clone();
        let root = root.join("bucket");
        let thread = std::thread::spawn(move || {
            while !ending.load(std::sync::atomic::Ordering::SeqCst) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 8192];
                let (head, body) = loop {
                    let n = stream.read(&mut buffer).unwrap();
                    assert_ne!(n, 0);
                    request.extend_from_slice(&buffer[..n]);
                    if let Some(at) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&request[..at]).to_string();
                        let length: usize = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|n| n.trim().parse().unwrap())
                            })
                            .unwrap();
                        if request.len() >= at + 4 + length {
                            break (head, request[at + 4..at + 4 + length].to_vec());
                        }
                    }
                };
                let path = head
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .trim_start_matches('/');
                link_cloud_contract::upload::check_object(path).unwrap();
                let path = root.join(path);
                let status = if path.exists() {
                    412
                } else if !head.to_ascii_lowercase().contains("if-none-match: *") {
                    403
                } else {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, body).unwrap();
                    if path.file_name().unwrap() == "output.tar" {
                        assert!(
                            Command::new("tar")
                                .arg("-xf")
                                .arg(&path)
                                .arg("-C")
                                .arg(path.parent().unwrap())
                                .status()
                                .unwrap()
                                .success()
                        );
                    }
                    200
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} Result\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            }
        });
        Self {
            url,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for ResultServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
