//! A project's dot files that run outside the sandbox, read only at the
//! top of every folder a sandbox writes, unless its policy names the
//! folder in `write_dot_files` (a spec's `[files] write_dot_files`).
//!
//! What's written there runs later as you, outside the sandbox, without
//! your running the project itself:
//!
//! - `.git`, all of it: its settings (`core.fsmonitor`, `core.sshCommand`,
//!   aliases, filters) run on any git command, its hooks on a commit, a
//!   checkout or a push, and a file in it (`commondir`) can point git at
//!   settings elsewhere. As a file (a worktree's or a submodule's), it
//!   says where git finds them.
//! - `.envrc`, which direnv runs when you `cd` into the folder.
//! - `.vscode`, whose tasks and settings your editor runs when it opens
//!   the folder, and `.idea`, JetBrains IDEs' run configurations.
//! - The hooks folder git's settings name in the folder (`core.hooksPath`,
//!   such as husky's `.husky/_`), and settings files they include from it.
//!   One below the top is read only, and each folder above it pinned
//!   (`scripts` for `scripts/hooks`): a folder above a mount could be
//!   renamed and made again. husky's `.husky` is kept whole, below.
//! - What a hook manager run from a stub in `.git/hooks` reads from the
//!   top of the folder ([`hook_managers`]: pre-commit's and lefthook's
//!   settings, husky's folder), where it's there.
//!
//! On Linux each is mounted read only over the folder's own bind; one that
//! isn't there is made empty for the run (a folder, or for an included
//! settings file, a file) and removed after ([`crate::linux::mounts`]),
//! but for `.envrc`, which direnv won't run until it's allowed. In Link's
//! own data folders (`~/Link/Agents`, `~/Link/Teams`), where no person's
//! project lives, none is made: one a program in the sandbox makes is
//! removed when its run ends, and the file tool never writes one. A link
//! there is refused: what it points at could be swapped. On macOS,
//! Seatbelt refuses writing each path, there or not, matched without case
//! as the Mac's disks are.

use std::path::{Component, Path, PathBuf};

/// The names at the top of a folder that run outside the sandbox, as
/// the sandbox's contract lists them (`dot_files.toml`).
pub fn names() -> &'static [String] {
    &link_sandbox_contract::dot_files::get().names
}

/// What hook managers read from the top of a folder, and run from a stub
/// git's hooks start: pre-commit's settings, lefthook's settings and
/// scripts, husky's folder. Kept read only where they're there; none is
/// made where it isn't.
pub fn hook_managers() -> &'static [String] {
    &link_sandbox_contract::dot_files::get().hook_managers
}

/// Whether `name` (one part of a path) is one of [`names`], whatever its
/// case, as a Mac's disk finds it.
pub fn named(name: &str) -> bool {
    link_sandbox_contract::dot_files::named(name)
}

/// What a path at the top of a folder is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Folder,
    File,
    /// A link (Linux refuses the sandbox).
    Link,
}

/// One path that runs outside, at the top of a folder the sandbox writes.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The folder the sandbox writes.
    pub folder: PathBuf,
    /// Its path in the folder: `.git`, `.husky/_`.
    pub rel: PathBuf,
    /// What it is; when it isn't there, what Linux makes in its place.
    pub kind: Kind,
    /// Whether it's there.
    pub exists: bool,
    /// A folder above a hooks folder or included file below the top:
    /// kept from being renamed or removed, not read only, so what else is
    /// in it stays the sandbox's to write.
    pub pin: bool,
    /// Whether its folder is in Link's own data folders
    /// (`~/Link/Agents`, `~/Link/Teams`), where no person's project
    /// lives: one that isn't there is never made, but removed when a
    /// run ends ([`Entry::sweeps`]).
    pub swept: bool,
}

impl Entry {
    pub fn path(&self) -> PathBuf {
        self.folder.join(&self.rel)
    }

    /// Whether Linux puts an empty placeholder here while a sandbox runs:
    /// it isn't there, but for `.envrc`, which direnv runs only once you
    /// `direnv allow` it, so a new one runs nothing by itself.
    /// Never in Link's data folders, which are swept instead.
    pub fn placeholder(&self) -> bool {
        !self.swept && self.missing() && self.rel != Path::new(".envrc")
    }

    /// Whether Linux removes it when a run ends, if a program in the
    /// sandbox made it: in Link's data folders, where it wasn't there,
    /// `.envrc` too.
    pub fn sweeps(&self) -> bool {
        self.swept && self.missing()
    }

    /// Not there, and nothing of a folder kept above it: what a
    /// placeholder or a sweep is for.
    fn missing(&self) -> bool {
        !self.exists && !self.pin && self.kind != Kind::Link
    }

    /// Whether Linux mounts it read only: it's there, or a placeholder is.
    pub fn mounted(&self) -> bool {
        self.exists || self.placeholder()
    }
}

/// What runs outside from the top of `folder` (a folder the sandbox
/// writes, resolved), `home` the user's: [`names`], and the hooks folder
/// and included settings git's settings name in it. `swept`: `folder` is
/// in Link's data folders ([`Entry::swept`]).
pub fn of(folder: &Path, home: &Path, swept: bool) -> Vec<Entry> {
    let mut entries = of_folder(folder, home);
    for entry in &mut entries {
        entry.swept = swept;
    }
    entries
}

fn of_folder(folder: &Path, home: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = names()
        .iter()
        .map(|name| entry(folder, Path::new(name), Kind::Folder))
        .collect();
    for name in hook_managers() {
        let next = entry(folder, Path::new(name), Kind::File);
        if next.exists {
            entries.push(next);
        }
    }
    let git = folder.join(".git");
    let mut configs: Vec<PathBuf> = vec!["/etc/gitconfig".into()];
    match std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()) {
        Some(xdg) => configs.push(Path::new(&xdg).join("git/config")),
        None => configs.push(home.join(".config/git/config")),
    }
    configs.push(home.join(".gitconfig"));
    let ours = git.symlink_metadata().is_ok_and(|m| m.is_dir());
    let mut hooks = Vec::new();
    for config in &configs {
        let (paths, _) = settings(&std::fs::read_to_string(config).unwrap_or_default());
        hooks.extend(paths);
    }
    // The repository's own settings, and what they include from the
    // folder: read only, they're what git reads when you run it.
    let mut includes = Vec::new();
    if ours {
        let mut queue = vec![git.join("config"), git.join("config.worktree")];
        let mut seen = Vec::new();
        while let Some(file) = queue.pop() {
            if seen.contains(&file) || seen.len() > 16 {
                continue;
            }
            seen.push(file.clone());
            let (paths, included) = settings(&std::fs::read_to_string(&file).unwrap_or_default());
            hooks.extend(paths);
            let base = file.parent().unwrap_or(folder).to_path_buf();
            for include in included {
                let Some(at) = place(&include, &base, home) else {
                    continue;
                };
                if let Ok(rel) = at.strip_prefix(folder)
                    && !rel.as_os_str().is_empty()
                    && !covered(rel)
                {
                    includes.push(rel.to_path_buf());
                }
                queue.push(at);
            }
        }
    }
    for hook in hooks {
        // A relative hooks folder is from the top of the working tree.
        let Some(at) = place(&hook, folder, home) else {
            continue;
        };
        if let Ok(rel) = at.strip_prefix(folder)
            && !rel.as_os_str().is_empty()
            && !covered(rel)
        {
            below_the_top(&mut entries, folder, rel, Kind::Folder);
        }
    }
    for rel in includes {
        below_the_top(&mut entries, folder, &rel, Kind::File);
    }
    entries
}

/// Adds `rel` (a hooks folder, or an included file), read only. Below the
/// top, each folder above it is pinned, so none can be renamed and made
/// again with what the sandbox wants there; where one of them isn't there
/// (a placeholder would be made, which could be), the first part is kept
/// whole instead.
fn below_the_top(entries: &mut Vec<Entry>, folder: &Path, rel: &Path, kind: Kind) {
    let mut push = |next: Entry| {
        if !entries.iter().any(|e| e.rel == next.rel) {
            entries.push(next);
        }
    };
    let parts: Vec<_> = rel.components().collect();
    let above: Vec<PathBuf> = (1..parts.len())
        .map(|n| parts[..n].iter().collect())
        .collect();
    let there = |p: &PathBuf| folder.join(p).symlink_metadata().is_ok_and(|m| m.is_dir());
    if !above.iter().all(there) {
        return push(entry(folder, &top(rel), Kind::Folder));
    }
    for p in above {
        push(Entry {
            pin: true,
            ..entry(folder, &p, Kind::Folder)
        });
    }
    push(entry(folder, rel, kind));
}

/// The first part of `rel`, a path in a folder: what's kept read only
/// for a path below the top whose folders aren't all there.
fn top(rel: &Path) -> PathBuf {
    rel.components()
        .next()
        .map(|first| PathBuf::from(first.as_os_str()))
        .unwrap_or_default()
}

/// Whether `rel` (a path in a folder) is in one of [`names`] already.
fn covered(rel: &Path) -> bool {
    rel.components()
        .next()
        .is_some_and(|first| named(&first.as_os_str().to_string_lossy()))
}

/// `rel` in `folder` as it is now; `missing`, what to make when it isn't
/// there. A link on the way to it is a link.
fn entry(folder: &Path, rel: &Path, missing: Kind) -> Entry {
    let mut at = folder.to_path_buf();
    let mut kind = missing;
    let mut exists = true;
    for part in rel.components() {
        at.push(part);
        match at.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                kind = Kind::Link;
                break;
            }
            Ok(meta) if meta.is_dir() => kind = Kind::Folder,
            Ok(_) => kind = Kind::File,
            Err(_) => {
                kind = missing;
                exists = false;
                break;
            }
        }
    }
    Entry {
        folder: folder.to_path_buf(),
        rel: rel.to_path_buf(),
        kind,
        exists,
        pin: false,
        swept: false,
    }
}

/// A path from git's settings, as git reads it: `~/` from `home`, a
/// relative one from `base`, every `.` and `..` taken out. None for one
/// git would read from elsewhere (`~user/`, an empty one).
fn place(text: &str, base: &Path, home: &Path) -> Option<PathBuf> {
    if text.is_empty() || (text.starts_with('~') && !text.starts_with("~/") && text != "~") {
        return None;
    }
    let path = match text.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if text == "~" => home.to_path_buf(),
        None => base.join(text),
    };
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    Some(out)
}

/// What a git settings file (`text`) says of where git finds what it
/// runs: each `core.hooksPath`, and each settings file it includes
/// (`include.path`, `includeIf.<condition>.path`), as written.
pub fn settings(text: &str) -> (Vec<String>, Vec<String>) {
    let (mut hooks, mut includes) = (Vec::new(), Vec::new());
    let mut section = String::new();
    for line in text.lines() {
        let mut line = line.trim();
        if let Some(rest) = line.strip_prefix('[') {
            let Some(end) = rest.find(']') else {
                continue;
            };
            // `[core]`, `[includeIf "gitdir:~/x/"]`, the old `[core.sub]`:
            // only the section's name matters here.
            let header = &rest[..end];
            let name = header
                .split(|c: char| c.is_whitespace() || c == '.' || c == '"')
                .next()
                .unwrap_or_default();
            section = name.to_ascii_lowercase();
            line = rest[end + 1..].trim();
            if line.is_empty() {
                continue;
            }
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = unquote(value);
        match (section.as_str(), key.as_str()) {
            ("core", "hookspath") => hooks.push(value),
            ("include" | "includeif", "path") => includes.push(value),
            _ => {}
        }
    }
    (hooks, includes)
}

/// A git setting's value as git reads it: comments (`#`, `;`) outside
/// quotes dropped, quotes taken off, `\"`, `\\`, `\t` and `\n` read.
fn unquote(value: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    let mut chars = value.trim().chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '#' | ';' if !quoted => break,
            '\\' => match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            },
            other => out.push(other),
        }
    }
    out.trim_end().to_string()
}

/// An error for a link in `entries` (those of folders that exist),
/// which the sandbox could point elsewhere: Linux can't keep a path read
/// only through it.
pub fn refuse_links(entries: &[Entry]) -> anyhow::Result<()> {
    for entry in entries.iter().filter(|e| e.folder.is_dir()) {
        anyhow::ensure!(
            entry.kind != Kind::Link,
            "{} is a link: the sandbox keeps what runs outside it read only there (git's \
             settings and hooks, .envrc, your editor's), and can't through a link. Make it a \
             folder or file, or let it write them: write_dot_files (--files.write_dot_files {})",
            entry.path().display(),
            entry.folder.display()
        );
    }
    Ok(())
}

/// Of `entries` (those of folders that exist), what Linux mounts read
/// only, each to have while the sandbox runs ([`crate::linux::mounts`]):
/// made empty where it isn't there, and held where it's one another run
/// made, so that run doesn't remove it from under this one.
pub fn wanted(entries: &[Entry]) -> Vec<crate::linux::mounts::Wanted> {
    entries
        .iter()
        .filter(|e| e.folder.is_dir() && e.mounted() && !e.pin && e.kind != Kind::Link)
        .map(|e| crate::linux::mounts::Wanted {
            folder: e.folder.clone(),
            rel: e.rel.clone(),
            file: e.kind == Kind::File,
        })
        .collect()
}

/// Of `entries` (those of folders that exist), what Linux removes when a
/// run ends where a program in the sandbox made it ([`Entry::sweeps`]),
/// each as a folder and its path there.
pub fn swept(entries: &[Entry]) -> Vec<crate::linux::mounts::Wanted> {
    entries
        .iter()
        .filter(|e| e.folder.is_dir() && e.sweeps())
        .map(|e| crate::linux::mounts::Wanted {
            folder: e.folder.clone(),
            rel: e.rel.clone(),
            file: e.kind == Kind::File,
        })
        .collect()
}

/// A Seatbelt regex matching `rel` in `folder`, and what's in it (or, for
/// a file, it alone), whatever the case of `rel`'s letters: a Mac's disk
/// finds `.GIT` at `.git`, and folds `ſ` to `s` and the Kelvin sign to `k`.
pub fn seatbelt_regex(folder: &Path, rel: &Path, file: bool) -> String {
    let mut out = String::from("^");
    out += &escape(&folder.display().to_string());
    out.push('/');
    for c in rel.display().to_string().chars() {
        match c {
            's' | 'S' => out += "(s|S|\u{17f})",
            'k' | 'K' => out += "(k|K|\u{212a})",
            c if c.is_ascii_alphabetic() => {
                out.push('[');
                out.push(c.to_ascii_lowercase());
                out.push(c.to_ascii_uppercase());
                out.push(']');
            }
            c => out += &escape(&c.to_string()),
        }
    }
    out += match file {
        true => "$",
        false => "(/|$)",
    };
    out
}

/// `text` for a Seatbelt regex: its special characters escaped, and a
/// quote, which would end the profile's string, as any one character.
fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '"' => out.push('.'),
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The folder, kept while the test holds it, and its canonical path.
    fn temp(name: &str) -> (link_test_kit::TempDir, PathBuf) {
        let kept = link_test_kit::TempDir::new(&format!("dotfiles-{name}"));
        let dir = kept.canonicalize().unwrap();
        (kept, dir)
    }

    #[test]
    fn git_settings_say_where_hooks_and_included_settings_are() {
        let text = r#"
[core]
	repositoryformatversion = 0
	hooksPath = .husky/_ # husky's
[include]
	path = "../.gitconfig.project"
[includeIf "gitdir:~/work/"]
	path = ~/work.gitconfig
[CORE] HOOKSPATH = "with \"quotes\""
[alias]
	path = not-a-path
"#;
        let (hooks, includes) = settings(text);
        assert_eq!(hooks, [".husky/_", "with \"quotes\""]);
        assert_eq!(includes, ["../.gitconfig.project", "~/work.gitconfig"]);
    }

    #[test]
    fn a_project_has_its_dot_files_and_what_its_settings_name() {
        let (_kept, dir) = temp("project");
        let home = dir.join("home");
        let folder = dir.join("app");
        std::fs::create_dir_all(folder.join(".git/hooks")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            folder.join(".git/config"),
            "[core]\n\thooksPath = .husky/_\n[include]\n\tpath = ../.gitconfig.project\n\tpath = ../../elsewhere\n",
        )
        .unwrap();
        std::fs::write(folder.join(".envrc"), "").unwrap();
        let entries = of(&folder, &home, false);
        let found: Vec<(String, Kind, bool)> = entries
            .iter()
            .map(|e| (e.rel.display().to_string(), e.kind, e.exists))
            .collect();
        assert_eq!(
            found,
            [
                (".git".to_string(), Kind::Folder, true),
                (".envrc".into(), Kind::File, true),
                (".vscode".into(), Kind::Folder, false),
                (".idea".into(), Kind::Folder, false),
                (".husky".into(), Kind::Folder, false),
                (".gitconfig.project".into(), Kind::File, false),
            ]
        );
        // Each is had while it runs, what isn't there as a placeholder,
        // but a new .envrc, which direnv won't run unasked.
        let wanted: Vec<String> = wanted(&entries)
            .iter()
            .map(|w| format!("{} {}", w.rel.display(), w.file))
            .collect();
        assert_eq!(
            wanted,
            [
                ".git false",
                ".envrc true",
                ".vscode false",
                ".idea false",
                ".husky false",
                ".gitconfig.project true"
            ]
        );
        assert_eq!(entries.iter().filter(|e| e.placeholder()).count(), 4);
        std::fs::remove_file(folder.join(".envrc")).unwrap();
        let envrc = of(&folder, &home, false).remove(1);
        assert!(!envrc.placeholder() && !envrc.mounted());
    }

    #[test]
    fn a_path_below_the_top_is_kept_from_its_first_part_and_hook_managers_too() {
        let (_kept, dir) = temp("nested");
        let home = dir.join("home");
        let folder = dir.join("app");
        std::fs::create_dir_all(folder.join(".git")).unwrap();
        std::fs::create_dir_all(folder.join(".husky/_")).unwrap();
        std::fs::create_dir_all(folder.join("cfg")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            folder.join(".git/config"),
            "[core]\n\thooksPath = .husky/_\n[include]\n\tpath = ../cfg/inc.gitconfig\n",
        )
        .unwrap();
        std::fs::write(folder.join(".pre-commit-config.yaml"), "repos: []\n").unwrap();
        std::fs::write(folder.join("lefthook.yml"), "").unwrap();
        let entries = of(&folder, &home, false);
        let found: Vec<(String, Kind, bool, bool)> = entries
            .iter()
            .skip(names().len())
            .map(|e| (e.rel.display().to_string(), e.kind, e.exists, e.pin))
            .collect();
        // husky's scripts beside `.husky/_` read only; `cfg` pinned, so it
        // can't be renamed and made again, but what else is in it written.
        assert_eq!(
            found,
            [
                (".husky".to_string(), Kind::Folder, true, false),
                (".pre-commit-config.yaml".into(), Kind::File, true, false),
                ("lefthook.yml".into(), Kind::File, true, false),
                (".husky/_".into(), Kind::Folder, true, false),
                ("cfg".into(), Kind::Folder, true, true),
                ("cfg/inc.gitconfig".into(), Kind::File, false, false),
            ]
        );
        // A pinned folder is no placeholder, and needs none.
        assert!(wanted(&entries).iter().all(|w| w.rel != Path::new("cfg")));
        // A hook manager's file that isn't there is no placeholder.
        assert!(!entries.iter().any(|e| e.rel == Path::new("lefthook.yaml")));
    }

    #[test]
    fn a_link_is_refused_and_nothing_is_made_through_one() {
        let (_kept, dir) = temp("link");
        let folder = dir.join("app");
        std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        std::os::unix::fs::symlink(dir.join("elsewhere"), folder.join(".vscode")).unwrap();
        let entries = of(&folder, &dir, false);
        let vscode = entries
            .iter()
            .find(|e| e.rel == Path::new(".vscode"))
            .unwrap();
        assert_eq!(vscode.kind, Kind::Link);
        let err = refuse_links(&entries).unwrap_err().to_string();
        assert!(
            err.contains("is a link") && err.contains("--files.write_dot_files"),
            "{err}"
        );
        // A hooks folder under a link is a link too, never a placeholder.
        std::os::unix::fs::symlink(dir.join("elsewhere"), folder.join(".husky")).unwrap();
        let hooks = entry(&folder, Path::new(".husky/_"), Kind::Folder);
        assert_eq!(hooks.kind, Kind::Link);
        assert!(refuse_links(std::slice::from_ref(&hooks)).is_err());
        assert!(wanted(&[hooks]).is_empty());
    }

    #[test]
    fn paths_from_settings_outside_the_folder_are_left_out() {
        let (_kept, dir) = temp("outside");
        let folder = dir.join("app");
        std::fs::create_dir_all(folder.join(".git")).unwrap();
        std::fs::write(
            folder.join(".git/config"),
            "[core]\nhooksPath = ../../shared-hooks\n[include]\npath = /etc/other\n",
        )
        .unwrap();
        let entries = of(&folder, &dir, false);
        assert_eq!(entries.len(), names().len());
    }

    #[test]
    fn seatbelt_matches_any_case_and_escapes_the_folder() {
        let regex = seatbelt_regex(Path::new("/Users/a/my.app (1)"), Path::new(".git"), false);
        assert_eq!(regex, r"^/Users/a/my\.app \(1\)/\.[gG][iI][tT](/|$)");
        let regex = seatbelt_regex(Path::new("/a\"b"), Path::new(".vscode"), false);
        assert_eq!(regex, "^/a.b/\\.[vV](s|S|\u{17f})[cC][oO][dD][eE](/|$)");
        let regex = seatbelt_regex(Path::new("/a"), Path::new(".gitconfig.x"), true);
        assert!(regex.ends_with(r"[xX]$"), "{regex}");
        assert!(named(".GIT") && named(".Vscode") && !named(".github"));
    }

    /// In Link's data folders nothing is made for a name that isn't
    /// there: it's swept when a run ends instead, `.envrc` too; what is
    /// there is kept read only, as anywhere.
    #[test]
    fn in_a_data_folder_nothing_is_made_and_what_is_missing_is_swept() {
        let (_kept, dir) = temp("data");
        let folder = dir.join("home");
        std::fs::create_dir_all(folder.join(".idea")).unwrap();
        let entries = of(&folder, &dir, true);
        assert!(!entries.iter().any(Entry::placeholder));
        let rels = |list: Vec<crate::linux::mounts::Wanted>| -> Vec<String> {
            list.iter().map(|w| w.rel.display().to_string()).collect()
        };
        assert_eq!(rels(wanted(&entries)), [".idea"]);
        assert_eq!(rels(swept(&entries)), [".git", ".envrc", ".vscode"]);
        // Outside them, today's placeholders, and nothing swept.
        let entries = of(&folder, &dir, false);
        assert!(swept(&entries).is_empty());
        assert_eq!(rels(wanted(&entries)), [".git", ".vscode", ".idea"]);
    }
}
