//! Where a harness has spread outside its home: `lnk agent harness show`
//! lists it, `lnk agent harness delete --harness <harness>` deletes it.
//!
//! Boxed and sandboxed, a harness writes nothing outside its home. Traces
//! come from a harness installed by hand, or from running it with the
//! sandbox off. Two sources: the places the
//! adapter says its harness writes (`lnk-<name> traces`), and a shallow
//! look through the usual places for anything carrying the harness's name.
//! The look never opens folders macOS guards (Documents, Mail, ...).

use std::path::{Path, PathBuf};

use link_harness_contract::harness::traces::{Kind, Trace as Declared};

/// Folders looked through (one level deep) for names containing the
/// harness's, relative to the home folder; `/tmp` too.
const PLACES: &[&str] = &[
    "",
    ".local/bin",
    ".local/share",
    ".config",
    ".config/systemd/user",
    ".cache",
    "Library/LaunchAgents",
    "Library/Application Support",
    "Library/Caches",
    "Library/Logs",
    "Library/Preferences",
];

/// Something of a harness found outside its home.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Trace {
    pub path: PathBuf,
    /// Named by the adapter (`lnk-<name> traces`), rather than found by
    /// its name alone, which another program's could share.
    pub known: bool,
    /// What the adapter says is there, when it named it.
    pub kind: Option<Kind>,
}

/// Everything of harness `name` outside `keep` (its home and the rest of
/// `~/Link`, Link's own settings): the adapter's `traces` that exist, and
/// what the look through `home` and `tmp` finds. Sorted, without
/// duplicates or nested entries.
pub fn outside(
    name: &str,
    traces: &[Declared],
    home: &Path,
    tmp: &Path,
    keep: &[PathBuf],
) -> Vec<Trace> {
    let uid = uid();
    let mut found: Vec<Trace> = traces
        .iter()
        .map(|t| (t.at(home, &uid), t.kind))
        .filter(|(p, _)| p.symlink_metadata().is_ok())
        .map(|(path, kind)| Trace {
            path,
            known: true,
            kind: Some(kind),
        })
        .collect();
    let dirs = PLACES
        .iter()
        .map(|place| home.join(place))
        .chain([tmp.to_path_buf()]);
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if named_after(&entry.file_name().to_string_lossy(), name) {
                found.push(Trace {
                    path: entry.path(),
                    known: false,
                    kind: None,
                });
            }
        }
    }
    found.retain(|t| {
        !keep
            .iter()
            .any(|k| t.path.starts_with(k) || k.starts_with(&t.path))
    });
    // Known first, so a path found both ways stays known.
    found.sort_by(|a, b| a.path.cmp(&b.path).then(b.known.cmp(&a.known)));
    found.dedup_by(|a, b| a.path == b.path);
    // A path inside another found one goes with it.
    let all: Vec<PathBuf> = found.iter().map(|t| t.path.clone()).collect();
    found.retain(|t| {
        !all.iter()
            .any(|other| *other != t.path && t.path.starts_with(other))
    });
    found
}

/// Whether a file name carries the harness's name as whole words:
/// `otter`, `.otter`, `ai.otter.gateway.plist` and
/// `otter-state-locks-501` do, `myotternotes` doesn't.
fn named_after(file: &str, name: &str) -> bool {
    let words = |text: &str| -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let (file, name) = (words(file), words(name));
    !name.is_empty() && file.windows(name.len()).any(|w| w == name.as_slice())
}

/// Disk space used by `path`, in bytes (`du`), if it can tell.
pub fn size(path: &Path) -> Option<u64> {
    let output = link_secretive::process::command("du")
        .arg("-sk")
        .arg(path)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let kb: u64 = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(kb * 1024)
}

/// `812 MB`.
pub fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{} MB", b >> 20),
        b if b >= 1 << 10 => format!("{} KB", b >> 10),
        b => format!("{b} B"),
    }
}

fn uid() -> String {
    link_service::user_id().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_are_found_outside_the_box_only() {
        let home = link_test_kit::TempDir::new("traces");
        for dir in [
            ".otter/state",
            ".local/bin",
            "Library/LaunchAgents",
            ".lnk/data/agents/main/harnesses/otter/.otter",
            "Projects/otter-notes",
        ] {
            std::fs::create_dir_all(home.join(dir)).unwrap();
        }
        std::fs::write(home.join(".local/bin/otter"), "").unwrap();
        std::fs::write(home.join("Library/LaunchAgents/ai.Otter.gateway.plist"), "").unwrap();
        let traces = [
            Declared::new(Kind::Home, "~/.otter"),
            Declared::new(Kind::Config, "~/.otter/state"),
            Declared::new(Kind::Program, "~/.missing"),
        ];
        let tmp = home.join("tmp");
        std::fs::create_dir_all(tmp.join("otter-state-locks-501")).unwrap();
        std::fs::create_dir_all(home.join(".config/myotternotes")).unwrap();
        let found = outside("otter", &traces, &home, &tmp, &[home.join("Link")]);
        let rel: Vec<_> = found
            .iter()
            .map(|t| (t.path.strip_prefix(&home).unwrap().to_path_buf(), t.kind))
            .collect();
        // What the adapter named carries its kind; inside its home, its
        // settings go with the home.
        assert_eq!(
            rel,
            [
                (PathBuf::from(".local/bin/otter"), None),
                (".otter".into(), Some(Kind::Home)),
                ("Library/LaunchAgents/ai.Otter.gateway.plist".into(), None),
                ("tmp/otter-state-locks-501".into(), None),
            ]
        );
    }

    #[test]
    fn names_match_as_whole_words() {
        assert!(named_after("Heron", "heron"));
        assert!(named_after(".heron", "heron"));
        assert!(named_after("ai.heron.gateway.plist", "heron"));
        assert!(named_after("my-harness-cache", "my-harness"));
        assert!(!named_after("heronnotes", "heron"));
        assert!(!named_after("theron", "heron"));
        assert!(!named_after("my-harnesses", "my-harness"));
    }

    #[test]
    fn sizes_read() {
        assert_eq!(human(3 << 20), "3 MB");
        assert_eq!(human(1536 << 20), "1.5 GB");
    }
}
