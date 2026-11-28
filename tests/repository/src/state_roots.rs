//! Link's persistent private state uses `.lnk`, in the person's or a
//! harness's home: it never makes a hidden folder of another name. This
//! guard reads Rust's literal root declarations, and the paths written in
//! docs, scripts and settings, so each workspace runs it on its own root;
//! it cannot infer the destination of arbitrary runtime path arithmetic.

use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Token {
    value: String,
    string: bool,
    line: usize,
}

/// Read strings and surrounding Rust tokens without matching comments.
/// Raw strings remain whole, so a script or fixture isn't parsed as Rust.
fn tokens(text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let (mut at, mut line) = (0, 1);
    while at < bytes.len() {
        if bytes[at].is_ascii_whitespace() {
            line += usize::from(bytes[at] == b'\n');
            at += 1;
            continue;
        }
        if bytes[at..].starts_with(b"//") {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if bytes[at..].starts_with(b"/*") {
            at += 2;
            let mut depth = 1;
            while at < bytes.len() && depth > 0 {
                if bytes[at..].starts_with(b"/*") {
                    depth += 1;
                    at += 2;
                } else if bytes[at..].starts_with(b"*/") {
                    depth -= 1;
                    at += 2;
                } else {
                    line += usize::from(bytes[at] == b'\n');
                    at += 1;
                }
            }
            continue;
        }
        // A quote inside a character literal isn't a string boundary.
        // Lifetimes have no closing apostrophe immediately after one char.
        if bytes[at] == b'\'' {
            let end = if bytes.get(at + 1) == Some(&b'\\') {
                let offset = at + 2 + usize::from(bytes.get(at + 2) == Some(&b'\''));
                bytes[offset..]
                    .iter()
                    .position(|byte| *byte == b'\'')
                    .map(|end| offset + end)
            } else {
                text[at + 1..]
                    .chars()
                    .next()
                    .map(|ch| at + 1 + ch.len_utf8())
                    .filter(|end| bytes.get(*end) == Some(&b'\''))
            };
            if let Some(end) = end {
                at = end + 1;
                continue;
            }
        }
        let start_line = line;
        let mut quote = at;
        if bytes[at] == b'r' {
            quote += 1;
            while bytes.get(quote) == Some(&b'#') {
                quote += 1;
            }
        }
        let raw = quote > at && bytes.get(quote) == Some(&b'"');
        if bytes[at] == b'"' || raw {
            let hashes = if raw { quote - at - 1 } else { 0 };
            let start = if raw { quote + 1 } else { at + 1 };
            at = start;
            while at < bytes.len() {
                if !raw && bytes[at] == b'\\' {
                    at = (at + 2).min(bytes.len());
                } else if bytes[at] == b'"'
                    && (!raw
                        || bytes
                            .get(at + 1..at + 1 + hashes)
                            .is_some_and(|end| end.iter().all(|b| *b == b'#')))
                {
                    break;
                } else {
                    line += usize::from(bytes[at] == b'\n');
                    at += 1;
                }
            }
            found.push(Token {
                value: text[start..at].to_owned(),
                string: true,
                line: start_line,
            });
            at = (at + 1 + hashes).min(bytes.len());
        } else if bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_' {
            let start = at;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            found.push(Token {
                value: text[start..at].to_owned(),
                string: false,
                line,
            });
        } else {
            found.push(Token {
                value: (bytes[at] as char).to_string(),
                string: false,
                line,
            });
            at += 1;
        }
    }
    found
}

fn native(name: &str) -> bool {
    // These names are owned by the named program or OS, not Link.
    [
        ".aws",
        ".bash_profile",
        ".bashrc",
        ".cache",
        ".claude",
        ".codex",
        ".config",
        ".deepseek",
        ".env",
        ".envrc",
        ".vscode",
        ".idea",
        ".agents",
        ".git",
        ".gitconfig",
        ".hermes",
        ".local",
        ".openclaw",
        ".profile",
        ".signal-cli",
        ".ssh",
    ]
    .contains(&name)
}

/// Whether `component`, a file's or folder's name, is one Link gives its
/// own: `.lnk-…` or `.link-…` at its start, or `.lnk-…` at its end, as in
/// `x.service.lnk-off`; never `.lnk` itself, where they belong.
fn link_named(component: &str) -> bool {
    // One ending `-` is a prefix a temporary's name is made from.
    component != ".lnk" && (component.starts_with(".lnk-") || component.starts_with(".link-"))
}

/// The files and folders of Link's a path written in `value` makes outside
/// `.lnk`: each component after the last `.lnk/`, but a name made anew
/// each time (a placeholder in it, a temporary), and never a value that
/// isn't a path (a label, an option, a command).
fn files_outside(value: &str) -> Vec<String> {
    if value.contains([':', '=', ' ', '*']) {
        return Vec::new();
    }
    if value.split('/').any(|part| part == ".lnk") {
        return Vec::new();
    }
    let after = value;
    after
        .split('/')
        .filter(|c| link_named(c))
        .map(str::to_owned)
        .collect()
}

pub fn rust(file: &Path, text: &str) -> Vec<String> {
    let tokens = tokens(text);
    let native_adapter = file
        .ancestors()
        .any(|parent| parent.ends_with("interop/agents/harnesses"));
    let mut found = Vec::new();
    let mut skip_test = None;
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        // Test strings describe fixtures and refused paths, rather than
        // production declarations.
        if !token.string && token.value == "#" {
            let following: Vec<_> = tokens[index..]
                .iter()
                .take(10)
                .map(|t| t.value.as_str())
                .collect();
            if following.starts_with(&["#", "[", "cfg", "(", "test", ")", "]"])
                && following.get(7) == Some(&"mod")
            {
                skip_test = Some(depth);
            }
        }
        if !token.string {
            if token.value == "{" {
                depth += 1;
            } else if token.value == "}" {
                depth = depth.saturating_sub(1);
                if skip_test == Some(depth) {
                    skip_test = None;
                }
            }
            continue;
        }
        if skip_test.is_some() {
            continue;
        }
        let value = token.value.as_str();
        let explicit = value
            .strip_prefix("~/")
            .or_else(|| value.strip_prefix("$HOME/"))
            .or_else(|| value.strip_prefix("${HOME}/"));
        let previous: Vec<_> = tokens[..index]
            .iter()
            .rev()
            .take(12)
            .map(|t| t.value.as_str())
            .collect();
        let expression = if previous.starts_with(&["(", "!", "format"]) {
            &previous[3..]
        } else {
            previous.as_slice()
        };
        let expression = expression.strip_prefix(&["&"]).unwrap_or(expression);
        let joined = expression.starts_with(&["(", "join", "."])
            || expression.starts_with(&["(", "folder", "."]);
        let receiver = expression.get(3..).unwrap_or_default();
        let call = receiver.strip_prefix(&["?"]).unwrap_or(receiver);
        let at_home = joined
            && (receiver.first() == Some(&"home")
                || call.starts_with(&[")", "(", "home_dir"])
                || call.starts_with(&[")", "(", "home"])
                || call.starts_with(&[")", "(", "try_home_dir"]));
        // Hidden children of visible roots are refused even when their
        // names don't identify Link (.moving, .memory, or a new spelling).
        // Native product paths are that product's interface, not Link state.
        let path_call = joined || expression.starts_with(&["(", "with_file_name", "."]);
        if path_call
            && (!at_home || !value.starts_with('.'))
            && explicit.is_none()
            && !native_adapter
            && !value.contains([' ', ':', '='])
            && !value.split('/').any(|part| part == ".lnk")
        {
            for part in value.split('/') {
                if part.starts_with('.')
                    && part != "."
                    && part != ".."
                    && !native(part)
                    && !link_named(part)
                    && ![".lock", ".net", ".keys", ".spool", ".key"].contains(&part)
                {
                    found.push(format!("{}:{}: {part} is a hidden path outside .lnk; use a machine path or a visible name", file.display(), token.line));
                }
            }
        }
        if joined
            && [
                "installation_dir",
                "try_installation_dir",
                "link_dir",
                "try_link_dir",
            ]
            .iter()
            .any(|function| call.starts_with(&[")", "(", function]))
        {
            found.push(format!(
                "{}:{}: use a typed Home or Hidden location instead of joining a root to {value}",
                file.display(),
                token.line
            ));
            continue;
        }
        let name = explicit
            .unwrap_or(value)
            .split('/')
            .next()
            .unwrap_or_default();
        if !(explicit.is_some() || at_home)
            || !name.starts_with('.')
            || name == ".lnk"
            || native(name.split('{').next().unwrap_or_default())
        {
            for name in files_outside(value) {
                found.push(format!("{}: {name} {FILE}", file.display()));
            }
            continue;
        }
        found.push(format!(
            "{}:{}: {name} is a private root outside .lnk",
            file.display(),
            token.line
        ));
    }
    found
}

fn source_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            if ![
                "target",
                "node_modules",
                ".git",
                ".next",
                ".turbo",
                "tests",
                "fixtures",
                ".agents",
                ".claude",
                ".codex",
            ]
            .iter()
            .any(|name| path.ends_with(name))
            {
                source_files(&path, files);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && path.components().any(|part| part.as_os_str() == "src")
            && path.file_name().is_none_or(|name| name != "tests.rs")
        {
            files.push(path);
        }
    }
}

/// The places a written path names a home, or a session's workspace: the
/// folder after one is where Link would keep its state.
const ANCHORS: &[&str] = &["~/", "$HOME/", "${HOME}/", "/root/", "/workspace/"];

/// A path written in a page, script or setting that puts a Link-named
/// hidden folder in a home beside `.lnk`: `~/.lnk-extra`, `$HOME/.link`,
/// `/root/.lnk-test`. A name with an extension is a file, and a name with
/// a placeholder in it is a temporary sibling, as in Rust.
pub fn text(file: &Path, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut starts: Vec<usize> = ANCHORS
            .iter()
            .flat_map(|anchor| line.match_indices(anchor).map(|(at, a)| at + a.len()))
            .collect();
        // `/home/<user>/`, whoever the user is.
        for (at, _) in line.match_indices("/home/") {
            let rest = &line[at + "/home/".len()..];
            if let Some(slash) = rest.find('/') {
                let user = &rest[..slash];
                if !user.is_empty()
                    && user
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_<>$".contains(&b))
                {
                    starts.push(at + "/home/".len() + slash + 1);
                }
            }
        }
        for start in starts {
            let rest = &line[start..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || "._-{}<>".contains(c)))
                .unwrap_or(rest.len());
            let name = &rest[..end];
            let linked = name == ".link" || name.starts_with(".link-") || name.starts_with(".lnk-");
            let file_like = name.get(1..).unwrap_or_default().contains('.');
            if linked && !file_like && !name.contains('<') {
                found.push(format!(
                    "{}:{}: {name} is a private root outside .lnk",
                    file.display(),
                    index + 1
                ));
            }
        }
    }
    found
}

/// The problems of `found` against `listed`, a workspace's
/// `state-roots.toml` `kept`: the files and folders of Link's outside
/// `.lnk` that code still makes, each `<file>: <name>`. A finding it
/// doesn't list is a problem, and so is a listed one no longer found: the
/// list may shrink, never grow.
pub fn against(found: &[String], listed: &[String]) -> Vec<String> {
    let kept = |finding: &str, entry: &str| finding.starts_with(&format!("{entry} {FILE}"));
    let mut problems: Vec<String> = found
        .iter()
        .filter(|f| !listed.iter().any(|l| kept(f, l)))
        .cloned()
        .collect();
    for entry in listed {
        if !found.iter().any(|f| kept(f, entry)) {
            problems.push(format!(
                "{entry}: no longer made outside .lnk: take it out of state-roots.toml"
            ));
        }
    }
    problems
}

/// What a file or folder of Link's outside `.lnk` is called in a finding.
const FILE: &str = "is a file or folder of Link's outside .lnk";

/// Whether `path` is in a folder `skip` names, from the workspace's root.
fn skipped(root: &Path, path: &Path, skip: &[&str]) -> bool {
    skip.iter().any(|s| path.starts_with(root.join(s)))
}

/// Docs, scripts and settings: what a person copies a path from.
fn text_files(root: &Path, dir: &Path, skip: &[&str], files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if ![
                "target",
                "node_modules",
                ".git",
                ".next",
                ".turbo",
                "fixtures",
                ".agents",
                ".claude",
                ".codex",
            ]
            .contains(&name.as_ref())
                && !skipped(root, &path, skip)
            {
                text_files(root, &path, skip, files);
            }
        } else if path.extension().is_some_and(|ext| {
            ["md", "sh", "py", "toml", "yml", "yaml", "env"].contains(&ext.to_str().unwrap_or(""))
        }) {
            files.push(path);
        }
    }
}

/// Every private root outside `.lnk` in a workspace: its Rust in `code`
/// (each folder's `src/`, but its tests and fixtures, which may exercise
/// old installations), and every page, script and setting under `root`
/// but the folders `skip` names (a record of what was, kept as it was).
pub fn check(root: &Path, code: &[&str], skip: &[&str]) -> Vec<String> {
    let mut rust_files = Vec::new();
    for dir in code {
        source_files(&root.join(dir), &mut rust_files);
    }
    rust_files.retain(|file| !skipped(root, file, skip));
    let mut texts = Vec::new();
    text_files(root, root, skip, &mut texts);
    let shown = |path: &Path| path.strip_prefix(root).unwrap_or(path).to_path_buf();
    let read = |path: &Path| std::fs::read_to_string(path).unwrap_or_default();
    rust_files
        .iter()
        .flat_map(|path| rust(&shown(path), &read(path)))
        .chain(
            texts
                .iter()
                .flat_map(|path| text(&shown(path), &read(path))),
        )
        .collect()
}
