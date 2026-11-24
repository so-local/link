//! `lnk sandbox run`, `wrap` and `check` under bubblewrap: a program in
//! the sandbox writes only what it was given, and reads nothing of the home
//! folder, Link's settings, the network or the caller's environment.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use link_test_kit::TempDir;

/// A home with a secret in it and in Link's settings, and a work folder.
struct World {
    root: TempDir,
    home: std::path::PathBuf,
    work: std::path::PathBuf,
    /// Short, as a machine's `/tmp` and runtime folder are: its runs'
    /// `TMPDIR` and `XDG_RUNTIME_DIR`, where their proxies' and Link's
    /// sockets are made, under a socket path's limit.
    short: TempDir,
}

impl World {
    fn new(test: &str) -> Self {
        Self::in_dir(TempDir::new(&format!("run-{test}")))
    }

    /// Out of `/tmp`, which no spec reads or writes.
    fn for_specs(test: &str) -> Self {
        Self::in_dir(TempDir::under(
            Path::new(env!("CARGO_TARGET_TMPDIR")),
            &format!("run-{test}"),
        ))
    }

    fn in_dir(root: TempDir) -> Self {
        let home = root.join("home");
        let work = home.join("work");
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::create_dir_all(home.join(".lnk/safe")).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(home.join(".ssh/id_test"), "SECRET KEY").unwrap();
        std::fs::write(home.join(".lnk/safe/token"), "SECRET TOKEN").unwrap();
        let short = TempDir::short("run");
        for folder in ["tmp", "run"] {
            std::fs::create_dir_all(short.join(folder)).unwrap();
        }
        Self {
            root,
            home,
            work,
            short,
        }
    }

    fn lnk(&self, args: &[&str]) -> Command {
        let mut command =
            link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-sandbox"), &self.home);
        command
            .args(args)
            .env("PATH", "/usr/bin:/bin")
            .env("TMPDIR", self.short.join("tmp"))
            .env("XDG_RUNTIME_DIR", self.short.join("run"))
            .env("GITHUB_TOKEN", "SECRET ENV")
            .current_dir(&self.work);
        command
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

mod commands;
mod confined;
mod hidden;
mod secrets;
mod stops;
mod watch;

/// The dynamic loader here, so a probe's program can be run from memory
/// and through it; empty where there's none (`[commands]` needs it).
fn loader() -> Option<std::path::PathBuf> {
    ["/lib64/ld-linux-x86-64.so.2", "/lib/ld-linux-aarch64.so.1"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.exists())
}
