//! The guard on what `lnk` prints: only `link-format` writes to a
//! terminal or sets up logging. Outside it, in any code `lnk` runs, a
//! print macro, `dbg!`, a stdout or stderr taken by hand or by its file
//! (`/dev/stdout`, `/dev/tty`, its descriptor), an ANSI escape, a
//! backtick in a string, a log subscriber or `RUST_LOG` fails, with no
//! file excepted: a protocol, another program's output and a child's
//! stderr have `link_format::streams` for them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[Rule {
    id: "output",
    scope: Scope::Every,
    fix: "only link-format prints: write blocks to its Out, never println!, eprintln!, \
          print!, dbg!, io::stdout(), io::stderr(), /dev/stdout, /dev/tty, an ANSI escape \
          or a backtick in output; a protocol, another program's output and a child's \
          stderr go through link_format::streams; a daemon logs through \
          link_format::daemon::logs(), never its own tracing_subscriber, env_logger or \
          RUST_LOG",
    check: Check::Whole(output),
}];

/// The group folders of what `lnk` runs, wherever a workspace keeps
/// them, and the package that alone prints.
const GROUPS: &[&str] = &["core", "packages", "modules", "interop"];
const FORMAT: &str = "packages/format";

/// Whether a file is code `lnk` runs, outside `link-format`: in a group
/// folder, or in a crate that is a module of its own (`module.toml`),
/// and not what only tests and builds run: a test (a `tests.rs` module
/// among them), a fixture, an example, a build script, or a crate in
/// `never` ([`never_run`]).
fn guarded(tree: &Tree, never: &BTreeSet<PathBuf>, shown: &str) -> bool {
    let path = Path::new(shown);
    let named = |names: &[&str]| {
        path.components()
            .any(|c| matches!(c, Component::Normal(n) if names.iter().any(|d| n == *d)))
    };
    path.extension().is_some_and(|e| e == "rs")
        && (named(GROUPS) || module(tree, path))
        && !path.starts_with(FORMAT)
        && !crate_dir(tree, path).is_some_and(|dir| never.contains(&dir))
        && path
            .file_name()
            .is_none_or(|n| n != "build.rs" && n != "tests.rs")
        && !named(&["tests", "benches", "examples", "fixtures"])
}

/// The crate `path` is in: the nearest folder above it with a
/// `Cargo.toml`.
fn crate_dir(tree: &Tree, path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .map(|dir| tree.root.join(dir))
        .find(|dir| tree.files.contains_key(&dir.join("Cargo.toml")))
}

/// Whether the crate `path` is in is a module: it has a `module.toml`.
fn module(tree: &Tree, path: &Path) -> bool {
    crate_dir(tree, path).is_some_and(|dir| tree.files.contains_key(&dir.join("module.toml")))
}

/// The crates `lnk` never runs, by their folders: those only tests and
/// build scripts link, as the testing kit and `link-component-build`
/// are: a library, never a program, whose every crate linking it does so
/// among its dev- or build-dependencies, or is a test's or is one itself.
fn never_run(tree: &Tree) -> BTreeSet<PathBuf> {
    let manifests: Vec<(PathBuf, toml::Table)> = tree
        .files
        .iter()
        .filter(|(p, _)| p.file_name().is_some_and(|n| n == "Cargo.toml"))
        .filter_map(|(p, text)| Some((p.parent()?.to_path_buf(), text.parse().ok()?)))
        .collect();
    let dirs: BTreeMap<&str, &PathBuf> = manifests
        .iter()
        .filter_map(|(dir, m)| Some((m.get("package")?.get("name")?.as_str()?, dir)))
        .collect();
    // Each crate's links: who links it, and whether normally.
    let mut links: BTreeMap<&PathBuf, Vec<(&PathBuf, bool)>> = BTreeMap::new();
    for (dir, manifest) in &manifests {
        let targets = manifest.get("target").and_then(toml::Value::as_table);
        let tables = std::iter::once(manifest).chain(
            targets
                .into_iter()
                .flat_map(|t| t.values().filter_map(toml::Value::as_table)),
        );
        for table in tables {
            for (kind, normal) in [
                ("dependencies", true),
                ("dev-dependencies", false),
                ("build-dependencies", false),
            ] {
                let names = table.get(kind).and_then(toml::Value::as_table);
                for name in names.into_iter().flat_map(|d| d.keys()) {
                    if let Some(linked) = dirs.get(name.as_str()) {
                        links.entry(linked).or_default().push((dir, normal));
                    }
                }
            }
        }
    }
    let test = |dir: &Path| {
        dir.strip_prefix(&tree.root)
            .unwrap_or(dir)
            .components()
            .any(|c| {
                matches!(c, Component::Normal(n) if ["tests", "benches", "examples", "fixtures"]
                .iter()
                .any(|t| n == *t))
            })
    };
    // A program is run, linked or not: a module, or a crate with a binary.
    let program = |dir: &Path| {
        let has = |file: &str| tree.files.contains_key(&dir.join(file));
        has("module.toml")
            || has("src/main.rs")
            || tree
                .files
                .keys()
                .any(|f| f.starts_with(dir.join("src/bin")))
            || manifests
                .iter()
                .any(|(d, m)| d == dir && m.contains_key("bin"))
    };
    // From every linked library, take out each one something run links
    // normally, until none goes.
    let mut never: BTreeSet<PathBuf> = links
        .keys()
        .filter(|d| !program(d))
        .map(|d| (*d).clone())
        .collect();
    loop {
        let run: Vec<PathBuf> = never
            .iter()
            .filter(|dir| {
                links[dir]
                    .iter()
                    .any(|(by, normal)| *normal && !test(by) && !never.contains(*by))
            })
            .cloned()
            .collect();
        if run.is_empty() {
            return never;
        }
        for dir in run {
            never.remove(&dir);
        }
    }
}

fn output(tree: &Tree) -> Vec<Finding> {
    let never = never_run(tree);
    tree.files
        .iter()
        .filter_map(|(path, text)| {
            let shown = tree.shown(path);
            let n = sites(text);
            (guarded(tree, &never, &shown) && n > 0)
                .then(|| fail(format!("{shown}: {n} print sites outside link-format")))
        })
        .collect()
}

/// The escapes that style a terminal, as Rust source writes them.
const ESCAPES: &[&str] = &["\\x1b", "\\x1B", "\\u{1b}", "\\u{1B}", "\\u{001b}", "\\033"];

/// A stream reached by its file or descriptor, past `link_format::streams`.
const BY_FILE: &[&str] = &[
    "/dev/stdout",
    "/dev/stderr",
    "/dev/tty",
    "from_raw_fd(1)",
    "from_raw_fd(2)",
    "from_raw_fd(libc::STDOUT_FILENO)",
    "from_raw_fd(libc::STDERR_FILENO)",
    "borrow_raw(1)",
    "borrow_raw(2)",
];

/// What sets up logging by hand: a subscriber of its own, or `RUST_LOG`.
const LOGGING: &[&str] = &["tracing_subscriber", "env_logger", "\"RUST_LOG\""];

/// How many print sites `text` has outside its tests and comments: each
/// print macro, `dbg!`, stdout or stderr taken by hand, escape, string
/// holding a backtick and piece of logging set up by hand. A raw
/// string's contents (a shell script, a page) are text, not code, and
/// count for nothing.
pub fn sites(text: &str) -> usize {
    let mut n = 0;
    let mut state = State::Code;
    // The depth of a `#[cfg(test)]` item being skipped, once it opens.
    let mut skipping: Option<i64> = None;
    let mut test_next = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if state == State::Code {
            if trimmed.starts_with("//") {
                continue;
            }
            if let Some(depth) = skipping.as_mut() {
                *depth += braces(line);
                if *depth <= 0 {
                    skipping = None;
                }
                continue;
            }
            if trimmed.starts_with("#[cfg(test)]") {
                test_next = true;
                continue;
            }
            if test_next && !trimmed.starts_with("#[") {
                test_next = false;
                let depth = braces(line);
                if depth > 0 {
                    skipping = Some(depth);
                }
                continue;
            }
        }
        let (code, backticks) = scan(line, &mut state);
        n += backticks;
        n += ["print!(", "println!(", "dbg!("]
            .iter()
            .map(|m| code.matches(m).count())
            .sum::<usize>();
        n += streams(&code);
        n += BY_FILE
            .iter()
            .chain(LOGGING)
            .map(|m| code.matches(m).count())
            .sum::<usize>();
        n += ESCAPES
            .iter()
            .map(|e| code.matches(e).count())
            .sum::<usize>();
    }
    n
}

/// How many times `code` takes stdout or stderr: `io::stdout()` and its
/// `std::`, `tokio::` or `anstream::` forms, or a bare `stdout()` brought
/// in by `use`, but never a method such as `Command::stdout(..)` or
/// `child.stderr()`.
fn streams(code: &str) -> usize {
    ["stdout(", "stderr("]
        .iter()
        .flat_map(|m| code.match_indices(m))
        .filter(|(i, m)| {
            let before = &code[..*i];
            let called = code[i + m.len()..].starts_with(')');
            before.ends_with("io::")
                || (called
                    && !before.ends_with(['.', '_'])
                    && !before.ends_with(|c: char| c.is_alphanumeric()))
        })
        .count()
}

/// Where a scan is: in code, in a string (and whether it has held a
/// backtick), or in a raw string closed by `"` and this many `#`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Code,
    Str(bool),
    Raw(usize),
}

/// `line`'s code and strings, its comment and raw strings' contents left
/// out, and how many strings holding a backtick it closed.
fn scan(line: &str, state: &mut State) -> (String, usize) {
    let chars: Vec<char> = line.chars().collect();
    let mut code = String::new();
    let mut closed = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match *state {
            State::Raw(hashes) => {
                let end = c == '"' && (1..=hashes).all(|k| chars.get(i + k) == Some(&'#'));
                if end {
                    *state = State::Code;
                    i += hashes;
                }
            }
            State::Str(backtick) => {
                code.push(c);
                match c {
                    '\\' => {
                        code.extend(chars.get(i + 1));
                        i += 1;
                    }
                    '"' => {
                        closed += backtick as usize;
                        *state = State::Code;
                    }
                    '`' => *state = State::Str(true),
                    _ => {}
                }
            }
            State::Code => match c {
                '/' if chars.get(i + 1) == Some(&'/') => break,
                '"' => {
                    code.push(c);
                    *state = State::Str(false);
                }
                'r' if !chars
                    .get(i.wrapping_sub(1))
                    .is_some_and(|p| p.is_alphanumeric() || *p == '_') =>
                {
                    let hashes = chars[i + 1..].iter().take_while(|h| **h == '#').count();
                    match chars.get(i + 1 + hashes) {
                        Some('"') => {
                            *state = State::Raw(hashes);
                            i += hashes + 1;
                        }
                        _ => code.push(c),
                    }
                }
                '\'' => {
                    // A char literal: '"' or '\'' never opens a string.
                    let close = match chars.get(i + 1) {
                        Some('\\') => chars[i + 2..]
                            .iter()
                            .position(|c| *c == '\'')
                            .map(|p| i + 2 + p),
                        Some(_) if chars.get(i + 2) == Some(&'\'') => Some(i + 2),
                        _ => None,
                    };
                    code.push(c);
                    if let Some(close) = close {
                        i = close;
                    }
                }
                _ => code.push(c),
            },
        }
        i += 1;
    }
    (code, closed)
}

/// How far a line opens braces: `{` less `}`.
fn braces(line: &str) -> i64 {
    line.matches('{').count() as i64 - line.matches('}').count() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_are_prints_escapes_and_backticks_outside_tests() {
        let text = concat!(
            "//! `lnk` prints \"x\" here, in a comment\n",
            "fn main() {\n",
            "    println!(\"hi\"); eprint!(\"no\");\n",
            "    let red = \"\\x1b[31m\";\n",
            "    let tick = '\"'; let s = \"run `lnk up`\"; // `ok`\n",
            "    let plain = \"no tick\";\n",
            "}\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn t() { println!(\"`x`\"); }\n",
            "}\n",
            "fn after() { eprintln!(\"y\"); }\n",
        );
        assert_eq!(sites(text), 5);
    }

    #[test]
    fn dbg_and_streams_taken_by_hand_are_sites() {
        let text = concat!(
            "use std::io::{stdout, Write};\n",
            "fn main() {\n",
            "    dbg!(1);\n",
            "    let _ = std::io::stdout().lock();\n",
            "    let _ = io::stderr();\n",
            "    writeln!(stdout(), \"x\");\n",
            "    Command::new(\"ls\").stdout(Stdio::piped()).stderr(Stdio::null());\n",
            "    let _ = child.stdout(); let _ = my_stderr();\n",
            "}\n",
        );
        assert_eq!(sites(text), 4);
    }

    #[test]
    fn a_stream_by_its_file_or_another_crate_is_a_site() {
        let text = concat!(
            "fn main() {\n",
            "    let _ = File::create(\"/dev/stdout\");\n",
            "    let _ = File::open(\"/dev/tty\");\n",
            "    let _ = unsafe { File::from_raw_fd(libc::STDERR_FILENO) };\n",
            "    let _ = anstream::stdout();\n",
            "    let _ = tokio::io::stderr();\n",
            "    let closing = fd > libc::STDERR_FILENO;\n",
            "    let _ = link_format::streams::protocol();\n",
            "}\n",
        );
        assert_eq!(sites(text), 5);
    }

    #[test]
    fn logging_set_up_by_hand_is_a_site() {
        let text = concat!(
            "use tracing_subscriber::EnvFilter;\n",
            "fn main() {\n",
            "    env_logger::init();\n",
            "    Command::new(\"x\").env(\"RUST_LOG\", \"debug\");\n",
            "    tracing::info!(\"tracing alone is fine\");\n",
            "    // tracing_subscriber in a comment is not\n",
            "}\n",
        );
        assert_eq!(sites(text), 3);
    }

    #[test]
    fn a_workspace_with_no_list_allows_none() {
        let temp = link_test_kit::TempDir::new("output-none");
        let root = temp.join("root");
        let _ = std::fs::remove_dir_all(&root);
        for (file, text) in [
            ("modules/x/src/main.rs", "fn main() { dbg!(1); }\n"),
            ("modules/x/src/quiet.rs", "fn quiet() {}\n"),
            (
                "packages/conformance/Cargo.toml",
                "[package]\nname = \"conformance\"\n[dependencies]\nkit = \"1\"\n",
            ),
            (
                "tests/e2e/Cargo.toml",
                "[package]\nname = \"e2e\"\n[dependencies]\nconformance = \"1\"\n",
            ),
            ("modules/y/Cargo.toml", "[package]\nname = \"y\"\n"),
            ("modules/y/src/main.rs", "fn main() {}\n"),
            (
                "tests/y/Cargo.toml",
                "[package]\nname = \"ty\"\n[dev-dependencies]\ny = \"1\"\n",
            ),
            ("large.toml", "[lines]\n"),
        ] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let tree = Tree::read(&root, false, &root.join("large.toml"));
        let found = output(&tree);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found.len(), 1);
        assert!(!found[0].warning && found[0].text.starts_with("modules/x/src/main.rs: 1 "));
    }

    #[test]
    fn a_raw_strings_contents_are_text() {
        let text = concat!(
            "const SCRIPT: &str = r#\"\n",
            "echo \"it's `here`\" \\\n",
            "printf '\\033[31m' # print!(\n",
            "\"#;\n",
            "fn main() { eprintln!(\"{}\", r\"`x`\"); let s = \"`y`\"; }\n",
        );
        assert_eq!(sites(text), 2);
    }

    #[test]
    fn only_code_lnk_runs_is_guarded() {
        let temp = link_test_kit::TempDir::new("output");
        let root = temp.join("root");
        let _ = std::fs::remove_dir_all(&root);
        for (file, text) in [
            ("ops/Cargo.toml", "[package]\nname = \"ops\"\n"),
            ("ops/module.toml", ""),
            ("qa/tool/Cargo.toml", "[package]\nname = \"tool\"\n"),
            ("packages/kit/Cargo.toml", "[package]\nname = \"kit\"\n"),
            ("packages/both/Cargo.toml", "[package]\nname = \"both\"\n"),
            (
                "modules/x/Cargo.toml",
                "[package]\nname = \"x\"\n[dev-dependencies]\nkit = \"1\"\n\
                 [build-dependencies]\nboth = \"1\"\n[target.'cfg(unix)'.dependencies]\nboth = \"1\"\n",
            ),
            (
                "packages/conformance/Cargo.toml",
                "[package]\nname = \"conformance\"\n[dependencies]\nkit = \"1\"\n",
            ),
            (
                "tests/e2e/Cargo.toml",
                "[package]\nname = \"e2e\"\n[dependencies]\nconformance = \"1\"\n",
            ),
            ("modules/y/Cargo.toml", "[package]\nname = \"y\"\n"),
            ("modules/y/src/main.rs", "fn main() {}\n"),
            (
                "tests/y/Cargo.toml",
                "[package]\nname = \"ty\"\n[dev-dependencies]\ny = \"1\"\n",
            ),
            ("large.toml", "[lines]\n"),
        ] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let tree = Tree::read(&root, false, &root.join("large.toml"));
        let never = never_run(&tree);
        let guarded = |shown: &str| guarded(&tree, &never, shown);
        assert!(guarded("modules/agents/agent/src/main.rs"));
        assert!(guarded("cloud-private/modules/cloud/src/main.rs"));
        assert!(guarded("ops/src/main.rs"));
        assert!(!guarded("qa/tool/src/main.rs"));
        assert!(!guarded("packages/format/src/out.rs"));
        assert!(!guarded("modules/agents/agent/tests/chat.rs"));
        assert!(!guarded(
            "modules/networking/relay/server/src/session/tests.rs"
        ));
        assert!(!guarded("packages/kit/src/lib.rs"));
        assert!(!guarded("packages/conformance/src/lib.rs"));
        assert!(guarded("modules/y/src/lib.rs"));
        assert!(guarded("packages/both/src/lib.rs"));
        assert!(!guarded("tests/docs/src/lib.rs"));
        assert!(!guarded("modules/agents/agent/README.md"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
