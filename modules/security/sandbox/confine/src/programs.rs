//! The programs a sandbox runs, judged against a spec's `[commands]`
//! and found by `lnk sandbox learn`: the parts that read no kernel's
//! report, used on Linux (`execs`) and macOS (`reports`) alike.

use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The folders any user can write, where no program a spec lists may be:
/// `/tmp`, `/var/tmp`, `/dev/shm` and this user's temp folder, resolved.
/// On Linux each is the sandbox's own, empty ([`crate::linux::bwrap`]);
/// on macOS only the sandbox's own names in `/tmp` are open.
pub fn temp_folders() -> Vec<PathBuf> {
    vec![
        crate::resolve(Path::new("/tmp")),
        crate::resolve(Path::new("/var/tmp")),
        crate::resolve(Path::new("/dev/shm")),
        crate::resolve(&std::env::temp_dir()),
    ]
}

/// Whether `program` (absolute, resolved) is one of `programs`: that
/// file, or in that folder.
pub fn listed(programs: &[PathBuf], program: &Path) -> bool {
    programs
        .iter()
        .any(|p| program == p || (p.is_dir() && program.starts_with(p)))
}

/// Whether `programs` (files, or folders of them) lets `file` run, as
/// Landlock judges it: that file, any link to it, or a file in a folder.
pub fn allows(programs: &[PathBuf], file: &Path) -> bool {
    if crate::programs::listed(programs, file) {
        return true;
    }
    let Ok(meta) = std::fs::metadata(file) else {
        return false;
    };
    programs.iter().any(|p| {
        std::fs::metadata(p)
            .is_ok_and(|m| m.is_file() && m.dev() == meta.dev() && m.ino() == meta.ino())
    })
}

/// What `lnk sandbox learn` found a run started.
#[derive(Debug, Default, PartialEq)]
pub struct Learned {
    /// The entries to add to `[commands] allow`, in the order they first
    /// ran, each with whether it's an interpreter (it runs any code).
    pub add: Vec<(String, bool)>,
    /// Programs it ran from where the sandbox may write, or a temp
    /// folder: never offered, since it could replace them.
    pub writable: Vec<PathBuf>,
    /// How many programs it ran, all told.
    pub ran: usize,
}

/// The programs `lines` (as [`Watch::Learn`] writes them) started that
/// `listed` (the spec's programs; None, any) doesn't allow, but for
/// `always` (Link's own, the loaders), as `[commands] allow` entries: a
/// name where `path` (the run's `PATH`) finds that file by it, else its
/// path. None in `write` (the folders the sandbox may write), a temp
/// folder, `/dev` or `/proc` is offered.
pub fn learned(
    lines: &str,
    listed: Option<&[PathBuf]>,
    always: &[PathBuf],
    write: &[PathBuf],
    path: &std::ffi::OsStr,
) -> Learned {
    let mut out = Learned::default();
    let mut seen = BTreeSet::new();
    let mut unsafe_places: Vec<PathBuf> = write.iter().map(|w| crate::resolve(w)).collect();
    unsafe_places.extend(crate::programs::temp_folders());
    unsafe_places.extend([
        PathBuf::from("/tmp"),
        "/var/tmp".into(),
        "/dev".into(),
        "/proc".into(),
    ]);
    let always: Vec<PathBuf> = always.iter().map(|p| crate::resolve(p)).collect();
    for line in lines.lines() {
        let Some((given, file)) = line.split_once('\t') else {
            continue;
        };
        let (given, file) = (PathBuf::from(given), PathBuf::from(file));
        if !seen.insert(file.clone()) {
            continue;
        }
        out.ran += 1;
        if allows(&always, &file) || listed.is_none_or(|listed| allows(listed, &file)) {
            continue;
        }
        if unsafe_places
            .iter()
            .any(|place| file.starts_with(place) || given.starts_with(place))
        {
            out.writable.push(given);
            continue;
        }
        let name = given.file_name().map(|n| n.to_string_lossy().into_owned());
        let by_name = name.as_ref().filter(|name| {
            link_files::find_program(name, path).is_some_and(|found| crate::resolve(&found) == file)
        });
        let entry = match by_name {
            Some(name) => name.clone(),
            None => given.display().to_string(),
        };
        if !out.add.iter().any(|(e, _)| *e == entry) {
            out.add
                .push((entry, interpreter(&given) || interpreter(&file)));
        }
    }
    out
}

/// Whether `program` is an interpreter, which runs any code it's given,
/// by its name: a shell, `node`, `python3.12`, `perl`, `awk`.
pub fn interpreter(program: &Path) -> bool {
    let Some(name) = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
    else {
        return false;
    };
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-');
    [
        "sh",
        "bash",
        "dash",
        "zsh",
        "ksh",
        "mksh",
        "fish",
        "csh",
        "tcsh",
        "busybox",
        "node",
        "nodejs",
        "deno",
        "bun",
        "python",
        "pypy",
        "ruby",
        "perl",
        "php",
        "lua",
        "luajit",
        "tclsh",
        "wish",
        "Rscript",
        "java",
        "awk",
        "gawk",
        "mawk",
        "nawk",
        "osascript",
        "pwsh",
        "julia",
        "guile",
        "racket",
        "elixir",
        "erl",
        "ash",
        "rbash",
        "toybox",
        "nu",
        "elvish",
        "R",
        "php-cgi",
        "jshell",
        "qjs",
    ]
    .contains(&base)
}
