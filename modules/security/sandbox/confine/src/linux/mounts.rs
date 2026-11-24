//! The empty placeholders Linux puts where a project's dot file isn't
//! ([`crate::dotfiles`]), so a read-only mount has something to cover:
//! made when a sandbox starts, and removed when it ends.
//!
//! `lnk-sandbox placeholders` stands in front of bubblewrap, as
//! the program a wrapped command starts with. It makes each one, runs
//! bubblewrap, waits, and removes what it made; `lnk sandbox run`, which
//! starts the wrapped command itself, does that in its own process
//! ([`held`]). Another sandbox on the
//! same folder may be using one at that moment, and removing a folder
//! another sandbox has mounted over undoes that mount, opening the path
//! to it. So each placeholder is kept in a list in Link's settings, with
//! the processes holding it (`placeholders.json`, under a lock): a run
//! that finds one there already holds it too, and one is removed only
//! once nobody alive holds it, and only while it's still the same empty
//! folder or file Link made (same device and inode, nothing in it). A
//! run killed outright leaves its own; the next run to end removes them.
//!
//! In Link's own data folders nothing is made ([`crate::dotfiles`]): each
//! name that wasn't there is kept in the same list as one to sweep, and
//! when a run ends, whatever a program made at each of its own is
//! removed, all it holds with it, never through a link. Another run
//! holding the same name doesn't stop that: what's made there is no
//! one's to keep. One left by a run killed outright is swept by the next
//! run to end.

use std::path::{Path, PathBuf};

use std::os::unix::fs::MetadataExt;

use anyhow::{Context, bail};
use link_files::nofollow::{Dir, Kind};
use serde::{Deserialize, Serialize};

/// One placeholder to have while a sandbox runs: `rel` in `folder`, an
/// empty folder, or for `file`, an empty file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub folder: PathBuf,
    pub rel: PathBuf,
    pub file: bool,
}

/// A placeholder Link made, or a path it sweeps, and who holds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Record {
    path: PathBuf,
    /// The placeholder's device and inode; 0 for a path swept.
    dev: u64,
    ino: u64,
    file: bool,
    /// A path swept when its run ends ([`sweep`]), never a placeholder.
    sweep: bool,
    holders: Vec<u32>,
}

/// `placeholders.json`: each placeholder and path swept, and who holds it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Listed {
    records: Vec<Record>,
}

link_files::specification!(Listed, include_str!("mounts.schema.toml"));

mod cli;
pub use cli::{Placeholders, wrapping};

/// Where in `argv` the command [`wrapping`] runs starts: bubblewrap's
/// own program, 0 when it isn't wrapped. (Its descriptors, such as
/// `--block-fd`'s, pass through this program unchanged.)
pub fn command_at(argv: &[String]) -> usize {
    let mut at = 0;
    loop {
        let args = &argv[at..];
        let boundary = if args.first().is_some_and(|x| x == "/usr/bin/sudo") {
            args.iter().position(|x| x == "--")
        } else if args
            .get(1)
            .is_some_and(|word| matches!(word.as_str(), "placeholders" | "worker"))
        {
            args.iter().position(|x| x == "--")
        } else {
            None
        };
        match boundary {
            Some(end) => at += end + 1,
            None => return at,
        }
    }
}

/// Runs `sandbox placeholders <args>`: has its placeholders, runs the
/// command, removes them; the command's exit status.
pub fn run(given: Placeholders) -> anyhow::Result<i32> {
    #[cfg(target_os = "linux")]
    // SAFETY: a plain prctl: SIGTERM here when whatever started this
    // goes, so its sandbox ends and its placeholders go with it.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM)
    };
    held(
        given,
        |program| link_secretive::process::carried(program),
        link_secretive::process::Terminal::Passed,
    )
}

/// What [`wrapping`] wrote for `exe` to run, when `argv` is that: the
/// arguments [`held`] takes.
pub fn stage<'a>(exe: &Path, argv: &'a [String]) -> Option<&'a [String]> {
    let (program, rest) = argv.split_first()?;
    (Path::new(program) == exe && rest.first()? == "placeholders").then(|| &rest[1..])
}

/// [`run`]'s work in the process that starts the sandbox, in place of a
/// process of its own in front of it: has the placeholders `args` names
/// ([`parse`]), pins bubblewrap's mount sources, starts its command as
/// `command` makes it, stands in front of it as `terminal` says, waits,
/// and removes them; the command's exit status.
pub fn held(
    given: Placeholders,
    command: impl FnOnce(&str) -> std::process::Command,
    terminal: link_secretive::process::Terminal,
) -> anyhow::Result<i32> {
    let cli::Parts {
        dir,
        wanted,
        swept,
        command: argv,
    } = given.parts()?;
    let me = std::process::id();
    let listed = !wanted.is_empty() || !swept.is_empty();
    // Every one held before anything runs, or nothing runs.
    if listed && let Err(err) = hold(&dir, &wanted, &swept, me) {
        let _ = release(&dir, me);
        return Err(err);
    }
    let status = (|| -> anyhow::Result<_> {
        #[cfg(target_os = "linux")]
        let (argv, pins, asked) = {
            let mut argv = argv;
            // The root-owned worker stage maps controller state first and
            // pins the resulting private views immediately before bubblewrap.
            // Pinning here would replace the paths its idmap plan must match;
            // it asks bubblewrap for its report itself.
            let worker = argv
                .first()
                .is_some_and(|exe| Path::new(exe) == link_sandbox_contract::box_workers::program())
                && argv.get(1).is_some_and(|word| word == "worker");
            match worker {
                true => (argv, None, None),
                false => {
                    let asked = super::session::ask(&mut argv)?;
                    let pins = super::pins::Pins::of(&mut argv)?;
                    (argv, Some(pins), Some(asked))
                }
            }
        };
        let mut command = command(&argv[0]);
        command.args(&argv[1..]);
        #[cfg(target_os = "linux")]
        if let Some(pins) = &pins {
            pins.pass_to(&mut command);
        }
        #[cfg(target_os = "linux")]
        if let Some(asked) = &asked {
            asked.pass_to(&mut command);
        }
        let started = command.spawn();
        #[cfg(target_os = "linux")]
        drop(pins);
        // Every stop passed on, the terminal's as `terminal` says, to
        // bubblewrap's sandbox once it names it ([`super::session`]): it
        // waits for the command to end, then removes the placeholders.
        let status = started.and_then(|mut child| {
            link_secretive::process::stand_in_front(child.id(), terminal);
            #[cfg(target_os = "linux")]
            if let Some(asked) = asked {
                asked.started(child.id());
            }
            link_secretive::process::wait_in_front(&mut child)
        });
        let status = status.with_context(|| format!("running {}", argv[0]))?;
        Ok(status)
    })();
    if listed {
        release(&dir, me)?;
    }
    let status = status?;
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return Ok(128 + signal);
    }
    Ok(status.code().unwrap_or(1))
}

/// The list in `dir`, under a lock for `change` (the folder itself,
/// locked, so nothing is left there when the list is empty), written
/// back after.
fn locked<T>(
    dir: &Path,
    change: impl FnOnce(&mut Vec<Record>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    use std::os::fd::AsRawFd;
    link_files::create_private_dir(dir).with_context(|| format!("making {}", dir.display()))?;
    let lock = std::fs::File::open(dir)?;
    // SAFETY: flock on a descriptor we hold; released when it closes.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        bail!(
            "locking {}: {}",
            dir.display(),
            std::io::Error::last_os_error()
        );
    }
    let list = dir.join("placeholders.json");
    // As read: None when there's no list, or one that can't be read (its
    // records then lost, as ever).
    let read: Option<Vec<Record>> = match std::fs::read(&list) {
        Ok(bytes) => Some(
            link_files::saved::parse_json::<Listed>(&bytes)
                .map(|listed| listed.records)
                .unwrap_or_default(),
        ),
        Err(_) => None,
    };
    let mut records = read.clone().unwrap_or_default();
    let out = change(&mut records);
    // Written back only when it changed: a run whose placeholders are all
    // there already, or which had none to make, rewrites nothing.
    let unchanged = match &read {
        Some(read) => *read == records && !records.is_empty(),
        None => records.is_empty(),
    };
    if !unchanged {
        match records.is_empty() {
            true => {
                let _ = std::fs::remove_file(&list);
            }
            false => link_files::write_private(
                &list,
                &link_files::saved::print_json(&Listed { records })?,
            )?,
        }
    }
    out
}

/// `path`, a folder, opened whole ([`Dir`]): what's made or removed in it
/// is by name in this very folder, so a folder above it swapped for a
/// link meanwhile changes nothing. Refused where its last part is a link,
/// or where a link on the way took it elsewhere.
fn open_folder(path: &Path) -> std::io::Result<Dir> {
    let folder = Dir::open_unlinked(path)?;
    let opened = folder.metadata()?;
    let there = std::fs::metadata(path)?;
    if path.canonicalize()? != path || (opened.dev(), opened.ino()) != (there.dev(), there.ino()) {
        return Err(std::io::Error::other(format!(
            "{} isn't where it was",
            path.display()
        )));
    }
    Ok(folder)
}

/// What a name in a folder is, never following a link: its kind, and for
/// a file or folder its device, inode and size, read from the handle it
/// was opened as (none, and no size, where it can't be).
struct Found {
    kind: Kind,
    dev: u64,
    ino: u64,
    size: u64,
}

/// `name` in `folder`, as it is ([`Found`]).
fn found(folder: &Dir, name: &std::ffi::OsStr) -> std::io::Result<Found> {
    let kind = folder.kind(name)?;
    let (dev, ino, size) = match kind {
        // One that can't be opened (not readable) is none Link made.
        Kind::File | Kind::Folder => match folder.open_entry(name).and_then(|f| f.metadata()) {
            Ok(meta) => (meta.dev(), meta.ino(), meta.len()),
            Err(_) => (0, 0, u64::MAX),
        },
        Kind::Link | Kind::Other => (0, 0, u64::MAX),
    };
    Ok(Found {
        kind,
        dev,
        ino,
        size,
    })
}

/// Makes `name` in `folder`: an empty file, or an empty folder; never
/// through a link, never over what's there.
fn make(folder: &Dir, name: &std::ffi::OsStr, file: bool) -> std::io::Result<()> {
    match file {
        true => folder.create_new(name, 0o644).map(drop),
        false => folder.create_new_folder(name, 0o755).map(drop),
    }
}

/// Has each of `wanted` for process `me`: each part of its path that
/// isn't there made empty (never through a link, each by name in its
/// folder opened as it was checked), and each one Link made before held
/// too; and each of `swept` kept as one to sweep when `me` ends.
pub fn hold(dir: &Path, wanted: &[Wanted], swept: &[Wanted], me: u32) -> anyhow::Result<()> {
    locked(dir, |records| {
        for sweep in swept {
            let path = sweep.folder.join(&sweep.rel);
            match records.iter_mut().find(|r| r.sweep && r.path == path) {
                Some(record) if !record.holders.contains(&me) => record.holders.push(me),
                Some(_) => {}
                None => records.push(Record {
                    path,
                    dev: 0,
                    ino: 0,
                    file: sweep.file,
                    sweep: true,
                    holders: vec![me],
                }),
            }
        }
        for want in wanted {
            let mut at = want.folder.clone();
            let mut folder = open_folder(&want.folder)
                .with_context(|| format!("opening {}", want.folder.display()))?;
            let parts: Vec<_> = want.rel.components().collect();
            for (i, part) in parts.iter().enumerate() {
                let name = part.as_os_str();
                at.push(name);
                let last = i + 1 == parts.len();
                let file = last && want.file;
                let found = match found(&folder, name) {
                    Ok(found) => found,
                    Err(_) => {
                        make(&folder, name, file).with_context(|| {
                            format!(
                                "making {} empty, so the sandbox can't write it",
                                at.display()
                            )
                        })?;
                        let found = found(&folder, name)?;
                        records.push(Record {
                            path: at.clone(),
                            dev: found.dev,
                            ino: found.ino,
                            file,
                            sweep: false,
                            holders: vec![me],
                        });
                        found
                    }
                };
                if found.kind == Kind::Link {
                    bail!(
                        "{} is a link: not making {} through it",
                        at.display(),
                        want.folder.join(&want.rel).display()
                    );
                }
                if let Some(record) = records
                    .iter_mut()
                    .find(|r| !r.sweep && r.path == at && r.dev == found.dev && r.ino == found.ino)
                    && !record.holders.contains(&me)
                {
                    record.holders.push(me);
                }
                if !last {
                    folder = folder
                        .folder(name)
                        .with_context(|| format!("opening {}", at.display()))?;
                }
            }
        }
        Ok(())
    })
}

/// Lets go of what process `me` holds, and removes every placeholder
/// nobody alive holds, if it's still the empty one Link made (checked
/// and removed by name in its folder, opened without following a link);
/// the deepest first, so a folder made for one inside it goes too. Each
/// path `me` swept, or nobody alive holds, is swept ([`sweep`]): an error
/// names what couldn't be removed.
pub fn release(dir: &Path, me: u32) -> anyhow::Result<()> {
    locked(dir, |records| {
        let mut sweeps = Vec::new();
        for record in records.iter_mut() {
            let held = record.holders.contains(&me);
            record.holders.retain(|p| *p != me && alive(*p));
            if record.sweep && (held || record.holders.is_empty()) {
                sweeps.push(record.path.clone());
            }
        }
        let swept = sweep(&sweeps);
        records.retain(|r| !(r.sweep && r.holders.is_empty()));
        let mut free: Vec<Record> = records
            .iter()
            .filter(|r| r.holders.is_empty())
            .cloned()
            .collect();
        records.retain(|r| !r.holders.is_empty());
        free.sort_by_key(|r| std::cmp::Reverse(r.path.components().count()));
        for record in free {
            let (Some(parent), Some(name)) = (record.path.parent(), record.path.file_name()) else {
                continue;
            };
            let Ok(folder) = open_folder(parent) else {
                continue;
            };
            let Ok(found) = found(&folder, name) else {
                continue;
            };
            if found.dev != record.dev || found.ino != record.ino {
                continue;
            }
            let _ = match record.file {
                true if found.kind == Kind::File && found.size == 0 => folder.remove_file(name),
                // Only while empty: removing a folder fails otherwise.
                false if found.kind == Kind::Folder => folder.remove_folder(name),
                _ => Ok(()),
            };
        }
        swept
    })
}

/// Removes what's at each of `paths` (in Link's data folders), all it
/// holds with it: its folder opened without following a link, and a
/// link there removed itself, never followed. Nothing there is nothing
/// to do. An error names each path left.
fn sweep(paths: &[PathBuf]) -> anyhow::Result<()> {
    let mut left = Vec::new();
    for path in paths {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            continue;
        };
        // A folder above it that's gone, or a link now, holds nothing of
        // it to remove.
        let Ok(folder) = open_folder(parent) else {
            continue;
        };
        match folder.remove_all(name) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => left.push(format!("{} ({err})", path.display())),
        }
    }
    anyhow::ensure!(
        left.is_empty(),
        "a sandboxed program made what a person's tools run outside the sandbox, and it \
         couldn't be removed: {}. Remove it before running git or opening the folder in an \
         editor there",
        left.join(", ")
    );
    Ok(())
}

/// Whether process `pid` is there.
fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only asks whether it could be sent.
    let sent = unsafe { libc::kill(pid as libc::pid_t, 0) };
    let there = sent == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    // One that ended but wasn't waited for (its parent killed too, where
    // nothing reaps orphans) holds nothing.
    there && !zombie(pid)
}

/// Whether process `pid` has ended, not yet waited for (Linux).
fn zombie(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    // `pid (name) state ...`: the name may hold spaces and parentheses.
    stat.rsplit_once(')')
        .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> link_test_kit::TempDir {
        let dir = link_test_kit::TempDir::new(&format!("placeholders-{name}"));
        std::fs::create_dir_all(dir.join("app")).unwrap();
        dir
    }

    fn want(dir: &Path, rel: &str, file: bool) -> Wanted {
        Wanted {
            folder: dir.join("app"),
            rel: rel.into(),
            file,
        }
    }

    /// A process id nobody has: past the kernel's largest.
    const GONE: u32 = 4_194_305;

    #[test]
    fn what_a_run_makes_goes_when_nobody_holds_it() {
        let dir = temp("held");
        let settings = dir.join("settings");
        let me = std::process::id();
        let wanted = [
            want(&dir, ".vscode", false),
            want(&dir, ".husky/_", false),
            want(&dir, ".x.conf", true),
        ];
        hold(&settings, &wanted, &[], me).unwrap();
        assert!(dir.join("app/.husky/_").is_dir() && dir.join("app/.x.conf").is_file());
        // Another run (gone by now) holds them too; this one ending keeps
        // them while it's alive, and they go once it's gone.
        hold(&settings, &wanted, &[], 1).unwrap();
        release(&settings, me).unwrap();
        assert!(dir.join("app/.vscode").is_dir(), "held by another");
        release(&settings, 1).unwrap();
        for gone in [".vscode", ".husky", ".x.conf"] {
            assert!(!dir.join("app").join(gone).exists(), "{gone}");
        }
        assert!(!settings.join("placeholders.json").exists());
    }

    #[test]
    fn only_the_empty_one_link_made_is_removed() {
        let dir = temp("theirs");
        let settings = dir.join("settings");
        // The user's own empty folder: never Link's to remove.
        std::fs::create_dir(dir.join("app/.idea")).unwrap();
        hold(
            &settings,
            &[want(&dir, ".idea", false), want(&dir, ".vscode", false)],
            &[],
            GONE,
        )
        .unwrap();
        // Something put in it since, and one replaced: both stay.
        std::fs::write(dir.join("app/.vscode/tasks.json"), "{}").unwrap();
        release(&settings, std::process::id()).unwrap();
        assert!(dir.join("app/.idea").is_dir());
        assert!(dir.join("app/.vscode/tasks.json").is_file());
        assert!(!settings.join("placeholders.json").exists());

        hold(&settings, &[want(&dir, ".git", false)], &[], GONE).unwrap();
        std::fs::remove_dir(dir.join("app/.git")).unwrap();
        std::fs::create_dir(dir.join("app/.git")).unwrap();
        // A run killed outright left it; the next one to end tidies up,
        // but not a folder that's someone else's now.
        release(&settings, std::process::id()).unwrap();
    }

    /// A project folder reached through a link on the way, or one whose
    /// last part is a link, gets nothing made in it.
    #[test]
    fn a_folder_reached_through_a_link_is_refused() {
        let dir = temp("on-the-way");
        std::os::unix::fs::symlink(dir.join("app"), dir.join("linked")).unwrap();
        let through = |at: PathBuf| Wanted {
            folder: at,
            rel: ".vscode".into(),
            file: false,
        };
        for at in [dir.join("linked"), dir.join("linked/../linked")] {
            assert!(
                hold(&dir.join("settings"), &[through(at.clone())], &[], GONE).is_err(),
                "{at:?}"
            );
        }
        std::fs::create_dir(dir.join("real")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("app/sub")).unwrap();
        let err = hold(
            &dir.join("settings"),
            &[through(dir.join("app/sub/."))],
            &[],
            GONE,
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("isn't where it was"), "{err:#}");
        assert!(!dir.join("app/.vscode").exists() && !dir.join("real/.vscode").exists());
    }

    /// `run` holds the placeholders itself only where the stage is its
    /// own program's.
    #[test]
    fn the_stage_is_found_only_for_its_own_program() {
        let exe = Path::new("/x/lnk-sandbox");
        let argv = wrapping(
            exe,
            Path::new("/s"),
            &[],
            &[],
            vec!["bwrap".into(), "--".into(), "sh".into()],
        );
        assert_eq!(stage(exe, &argv), Some(&argv[2..]));
        assert_eq!(stage(Path::new("/y/lnk-sandbox"), &argv), None);
        assert_eq!(stage(exe, &argv[2..]), None);
        assert_eq!(stage(exe, &[]), None);
    }

    #[test]
    fn nothing_is_made_through_a_link_and_the_arguments_read_back() {
        let dir = temp("link");
        std::fs::create_dir(dir.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join("app/.husky")).unwrap();
        let err = hold(
            &dir.join("settings"),
            &[want(&dir, ".husky/_", false)],
            &[],
            GONE,
        )
        .unwrap_err();
        assert!(err.to_string().contains("is a link"), "{err}");
        assert!(!dir.join("elsewhere/_").exists());

        let wanted = [want(&dir, ".git", false), want(&dir, ".inc", true)];
        let argv = wrapping(
            Path::new("/x/lnk-sandbox"),
            Path::new("/s"),
            &wanted[..1],
            &wanted[1..],
            vec!["bwrap".into(), "--".into(), "sh".into()],
        );
        assert_eq!(argv[command_at(&argv)], "bwrap");
        assert_eq!(command_at(&["bwrap".to_string(), "--".into()]), 0);
        let words = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
        let app = dir.join("app").display().to_string();
        assert_eq!(
            argv,
            words(&format!(
                "/x/lnk-sandbox placeholders --dir /s --wanted folder --wanted {app} --wanted .git \
                 --swept file --swept {app} --swept .inc -- bwrap -- sh"
            ))
        );
        let words_of = |list: &[Wanted]| -> Vec<String> {
            list.iter()
                .flat_map(|w| {
                    [
                        if w.file { "file" } else { "folder" }.to_string(),
                        w.folder.display().to_string(),
                        w.rel.display().to_string(),
                    ]
                })
                .collect()
        };
        let given = Placeholders {
            dir: "/s".into(),
            wanted: words_of(&wanted[..1]),
            swept: words_of(&wanted[1..]),
            command: vec!["bwrap".into(), "--".into(), "sh".into()],
        };
        let parts = given.parts().unwrap();
        assert_eq!(parts.dir, PathBuf::from("/s"));
        assert_eq!(
            (parts.wanted, parts.swept),
            (wanted[..1].to_vec(), wanted[1..].to_vec())
        );
        assert_eq!(parts.command, ["bwrap", "--", "sh"]);
        let worker: Vec<String> = [
            "/usr/local/lib/lnk/lnk-sandbox",
            "worker",
            "--plan",
            "/s/worker.json",
            "--",
            "/usr/bin/bwrap",
            "--",
            "sh",
        ]
        .map(str::to_string)
        .to_vec();
        let nested = wrapping(
            Path::new("/x/lnk-sandbox"),
            Path::new("/s"),
            &[],
            &[],
            worker,
        );
        assert_eq!(nested[command_at(&nested)], "/usr/bin/bwrap");
        for bad in [&["folder", "/a", "../x"][..], &["folder", "a", "b"][..]] {
            for swept in [false, true] {
                let words: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
                let (wanted, swept) = match swept {
                    true => (vec![], words),
                    false => (words, vec![]),
                };
                let given = Placeholders {
                    dir: "/s".into(),
                    wanted,
                    swept,
                    command: vec!["sh".into()],
                };
                assert!(given.parts().is_err(), "{bad:?}");
            }
        }
    }

    /// In Link's data folders, what a program made where a dot file wasn't
    /// goes when its run ends, all of it, even while another run holds
    /// the same path; nothing is made for it, and a link there goes
    /// itself, never what it points at.
    #[test]
    fn what_a_program_made_where_one_is_swept_goes_when_its_run_ends() {
        let dir = temp("swept");
        let settings = dir.join("settings");
        let me = std::process::id();
        std::fs::create_dir(dir.join("elsewhere")).unwrap();
        std::fs::write(dir.join("elsewhere/keep"), "kept").unwrap();
        let swept = [
            want(&dir, ".git", false),
            want(&dir, ".envrc", false),
            want(&dir, ".vscode", false),
        ];
        // Another run, alive, holds them too: this one ending still sweeps.
        hold(&settings, &[], &swept, 1).unwrap();
        hold(&settings, &[], &swept, me).unwrap();
        for made in [".git", ".envrc", ".vscode"] {
            assert!(!dir.join("app").join(made).exists(), "nothing made: {made}");
        }
        std::fs::create_dir_all(dir.join("app/.git/hooks")).unwrap();
        std::fs::write(dir.join("app/.git/hooks/pre-commit"), "#!/bin/sh\n").unwrap();
        std::fs::write(dir.join("app/.envrc"), "echo hi\n").unwrap();
        std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join("app/.vscode")).unwrap();
        release(&settings, me).unwrap();
        for gone in [".git", ".envrc", ".vscode"] {
            assert!(
                dir.join("app").join(gone).symlink_metadata().is_err(),
                "{gone}"
            );
        }
        assert!(
            dir.join("elsewhere/keep").is_file(),
            "never through the link"
        );
        // Pid 1 holds them still: listed until it ends.
        let list = std::fs::read_to_string(settings.join("placeholders.json")).unwrap();
        assert!(list.contains(".git"), "{list}");
    }

    /// A run killed outright never swept its own: the next run to end
    /// does, once nobody alive holds them.
    #[test]
    fn a_killed_runs_sweep_is_done_by_the_next_to_end() {
        let dir = temp("swept-killed");
        let settings = dir.join("settings");
        hold(&settings, &[], &[want(&dir, ".idea", false)], GONE).unwrap();
        std::fs::create_dir(dir.join("app/.idea")).unwrap();
        std::fs::write(dir.join("app/.idea/workspace.xml"), "<x/>").unwrap();
        release(&settings, std::process::id()).unwrap();
        assert!(!dir.join("app/.idea").exists());
        assert!(!settings.join("placeholders.json").exists());
    }
}
