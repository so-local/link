//! What a Linux sandbox sees of the machine outside its folders: the
//! system's programs and libraries, and of `/etc` only what programs need.

use std::path::Path;

/// What a Linux program sees of the machine outside its folders, read
/// only, those that exist: [`crate::system::SYSTEM`].
pub const SYSTEM: &[&str] = crate::system::SYSTEM;

/// What of `etc` (the machine's `/etc`) a Linux program sees, at
/// `/etc`, a tmpfs of its own made read only once every mount in it is
/// made ([`crate::linux::bwrap::args`]): [`crate::system::ETC`] read only, a
/// link as the same link, but for anything named
/// [`crate::system::ETC_HIDDEN`] in them, never shown.
pub fn args(etc: &Path, args: &mut Vec<String>) {
    let mut names: Vec<String> = Vec::new();
    for name in crate::system::ETC {
        match name.strip_suffix('*') {
            Some(prefix) => {
                let mut found: Vec<String> = std::fs::read_dir(etc)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| {
                        n.starts_with(prefix) && link_sandbox_client::plain_path(Path::new(n))
                    })
                    .collect();
                found.sort();
                names.extend(found);
            }
            None => names.push(name.to_string()),
        }
    }
    args.extend(["--tmpfs".into(), "/etc".into()]);
    for name in &names {
        etc_entry(&etc.join(name), &format!("/etc/{name}"), 0, args);
    }
}

/// `path` shown at `at`: a link as the same link, a hidden name not at
/// all, a folder holding one entry by entry, anything else bound read
/// only.
fn etc_entry(path: &Path, at: &str, depth: usize, args: &mut Vec<String>) {
    let hidden = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| crate::system::ETC_HIDDEN.contains(&n));
    if hidden || !link_sandbox_client::plain_path(path) {
        return;
    }
    match path.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => {
            if let Ok(target) = std::fs::read_link(path) {
                args.extend(["--symlink".into(), target.display().to_string(), at.into()]);
            }
        }
        Ok(meta) if meta.is_dir() && depth < 4 && holds_hidden(path, depth) => {
            args.extend(["--dir".into(), at.into()]);
            let mut children: Vec<_> = std::fs::read_dir(path)
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            children.sort();
            for child in children {
                etc_entry(
                    &path.join(&child),
                    &format!("{at}/{child}"),
                    depth + 1,
                    args,
                );
            }
        }
        // Too deep to look through: not shown, rather than shown whole.
        Ok(meta) if meta.is_dir() && depth >= 4 && holds_hidden(path, depth) => {}
        Ok(_) => {
            args.extend(["--ro-bind".into(), path.display().to_string(), at.into()]);
        }
        Err(_) => {}
    }
}

/// Whether folder `dir` holds a hidden name, at `depth` below `/etc`'s
/// entries, looked for no further than 8 down.
fn holds_hidden(dir: &Path, depth: usize) -> bool {
    if depth > 8 {
        return true;
    }
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let name = e.file_name();
            name.to_str()
                .is_some_and(|n| crate::system::ETC_HIDDEN.contains(&n))
                || (e.file_type().is_ok_and(|t| t.is_dir()) && holds_hidden(&e.path(), depth + 1))
        })
}
