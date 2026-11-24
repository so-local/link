//! Releases of `lnk` for a test, as an installer checks and installs
//! from them: served on this machine over plain HTTP (as a debug build
//! may call it), each `install.sh`, `VERSION` and `SHA256SUMS` signed
//! with the tests' release key, which a debug build trusts when
//! `LNK_TEST_RELEASE_SIGNERS` names it.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use crate::TempDir;

/// The script each release's `install.sh` is.
pub const INSTALL: &str = "#!/bin/sh\necho 'installing lnk from the test release'\n";

/// The archives each release holds besides, standing for `lnk`'s and a
/// module's for two processors: what's in them is never run.
pub const ARCHIVES: &[&str] = &[
    "lnk-core-x86_64-unknown-linux-musl.tar.gz",
    "lnk-agent-x86_64-unknown-linux-musl.tar.gz",
    "lnk-core-aarch64-unknown-linux-musl.tar.gz",
];

/// Served releases: the latest and each of `versions`.
pub struct Releases {
    _dir: TempDir,
    /// The folder they're served from, laid out as a bucket holding them
    /// is.
    pub root: PathBuf,
    /// `LNK_RELEASES_URL`.
    pub url: String,
    /// `LNK_TEST_RELEASE_SIGNERS`.
    pub signers: PathBuf,
}

impl Releases {
    /// The variables a test's installer is run with.
    pub fn env(&self) -> [(&str, &str); 2] {
        [
            ("LNK_RELEASES_URL", self.url.as_str()),
            ("LNK_TEST_RELEASE_SIGNERS", self.signers.to_str().unwrap()),
        ]
    }
}

/// The tests' keys, read where they are, never built into a program.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// A release of `version` in `dir`, signed with the tests' key.
fn release(dir: &Path, version: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("install.sh"), INSTALL).unwrap();
    std::fs::write(dir.join("VERSION"), format!("{version}\n")).unwrap();
    for archive in ARCHIVES {
        std::fs::write(dir.join(archive), format!("{archive} of {version}\n")).unwrap();
    }
    let sums = link_secretive::process::command("sha256sum")
        .args(["install.sh", "VERSION"])
        .args(ARCHIVES)
        .current_dir(dir)
        .output()
        .unwrap();
    std::fs::write(dir.join("SHA256SUMS"), sums.stdout).unwrap();
    let key = dir.join("key");
    std::fs::copy(fixtures().join("test-release-key"), &key).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&key, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let signed = link_secretive::process::command("ssh-keygen")
        .args(["-q", "-Y", "sign", "-n", "link-release", "-f"])
        .arg(&key)
        .arg(dir.join("SHA256SUMS"))
        .stdin(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(signed.success());
    std::fs::remove_file(key).unwrap();
}

/// The latest release (this lnk's version) and one of each of
/// `versions`, served until the test ends; none, and the test skips,
/// where `ssh-keygen`, which signs them, isn't installed
/// ([`crate::gate::installed`]).
pub fn serve(name: &str, versions: &[&str]) -> Option<Releases> {
    if !crate::gate::installed("ssh-keygen") {
        return None;
    }
    let dir = TempDir::new(&format!("box-release-{name}"));
    release(&dir.join("latest/download"), env!("CARGO_PKG_VERSION"));
    for version in versions {
        release(&dir.join(format!("download/v{version}")), version);
    }
    let signers = dir.join("signers");
    let public = std::fs::read_to_string(fixtures().join("test-release-key.pub")).unwrap();
    std::fs::write(
        &signers,
        format!("link-release namespaces=\"link-release\" {}", public.trim()),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let root = dir.to_path_buf();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                // The request line, then its headers to the blank line.
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let mut header = String::new();
                while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                    header.clear();
                }
                let path = line.split_whitespace().nth(1).unwrap_or("/");
                let file = root.join(path.trim_start_matches('/'));
                let reply = match (path.contains(".."), std::fs::read(&file)) {
                    (false, Ok(body)) => {
                        let mut reply = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        reply.extend(body);
                        reply
                    }
                    _ => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = stream.write_all(&reply);
            });
        }
    });
    Some(Releases {
        root: dir.to_path_buf(),
        _dir: dir,
        url,
        signers,
    })
}
