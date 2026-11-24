//! Tests keep to the testing kit where Clippy can't see it: what a test
//! may do is a call Clippy refuses (`clippy.toml`), and what it may not
//! leave out is a pattern of calls, read here from the source.
//!
//! Each [`Rule`] names a pattern, where it applies and what to use
//! instead. The files that still break one are listed under its name in
//! the workspace's `test-kit.toml`, a list that may shrink and never
//! grow: a rule with none listed is refused everywhere. Raw calls a test
//! keeps "until they move onto the kit" are counted, and the count may
//! only go down.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where a rule reads: every line of a file, or only a test's.
#[derive(Clone, Copy, PartialEq)]
pub enum Scope {
    All,
    Tests,
}

/// How a rule finds its pattern.
#[derive(Clone, Copy)]
pub enum Pattern {
    /// In one line's code, its comment left out.
    Line(fn(code: &str) -> bool),
    /// In one line's code, and the next line.
    Lines(fn(code: &str, next: &str) -> bool),
    /// In the code the rule reads, whole: a pattern across lines.
    Whole(fn(code: &str) -> bool),
}

/// A pattern the source may not hold, and what it holds instead.
pub struct Rule {
    /// Its name in `test-kit.toml`.
    pub name: &'static str,
    pub scope: Scope,
    pub pattern: Pattern,
    /// What to do instead.
    pub instead: &'static str,
}

/// Every rule, in the order they're said.
pub const RULES: &[Rule] = &[
    Rule {
        name: "printed-skips",
        scope: Scope::All,
        pattern: Pattern::Lines(printed_skip),
        instead: "prints its own skip and returns: skip through the testing kit's gate",
    },
    Rule {
        name: "require-variables",
        scope: Scope::All,
        pattern: Pattern::Line(|code| code.contains("\"LNK_REQUIRE_")),
        instead: "reads an LNK_REQUIRE_* variable: only the testing kit's gates do",
    },
    Rule {
        name: "port-probes",
        scope: Scope::All,
        pattern: Pattern::Whole(probes),
        instead: "finds a port free by binding 127.0.0.1:0 and letting it go: hold it (link_test_kit::Held, HeldNumber)",
    },
    Rule {
        name: "unexplained-ignores",
        scope: Scope::Tests,
        pattern: Pattern::Line(|code| code.trim() == "#[ignore]"),
        instead: "ignores a test without saying why: #[ignore = \"why\"]",
    },
    Rule {
        name: "elapsed-asserts",
        scope: Scope::Tests,
        pattern: Pattern::Whole(asserts_elapsed),
        instead: "proves something by elapsed time: wait for the event that shows it (Test Environments, Load and timing)",
    },
    Rule {
        name: "process-environment",
        scope: Scope::Tests,
        pattern: Pattern::Line(|code| {
            code.contains("env::set_var(") || code.contains("env::remove_var(")
        }),
        instead: "changes the test process's environment, which every test in it shares: give the child its own (link_test_kit::env::command)",
    },
    Rule {
        name: "poll-loops",
        scope: Scope::Tests,
        pattern: Pattern::Whole(polls),
        instead: "waits in a loop that sleeps: link_test_kit::wait (eventually, within, exited, accepting, appears)",
    },
    Rule {
        name: "temp-folders",
        scope: Scope::Tests,
        pattern: Pattern::Line(|code| code.contains("env::temp_dir()")),
        instead: "makes a folder in the temp folder by hand: link_test_kit::TempDir",
    },
    Rule {
        name: "tmp-paths",
        scope: Scope::Tests,
        pattern: Pattern::Line(|code| code.contains("\"/tmp/")),
        instead: "names a path in /tmp: link_test_kit::TempDir (TempDir::short for a socket)",
    },
    Rule {
        name: "runtime-folders",
        scope: Scope::Tests,
        pattern: Pattern::Whole(clears_without_runtime),
        instead: "clears a child's environment and gives it no runtime folder, so its sockets are in the machine-wide one every test shares: link_test_kit::env::command, or .env(\"XDG_RUNTIME_DIR\", link_test_kit::env::made_runtime(&home))",
    },
    Rule {
        name: "programs-by-hand",
        scope: Scope::Tests,
        pattern: Pattern::Whole(|code| statement_holds(code, "set_permissions(", &["0o7"])),
        instead: "makes a program runnable by hand: link_test_kit::write_program",
    },
];

/// What a raw call a test keeps says, until it's replaced by the kit's.
pub const PROMISE: &str = "reason = \"moves onto the testing kit\"";

/// Whether a statement of `text` that starts at `opens` holds each of
/// `holds`, however rustfmt splits it across lines: read without
/// whitespace, from `opens` to the statement's `;`, or the `}` of the
/// block it ends.
pub fn statement_holds(text: &str, opens: &str, holds: &[&str]) -> bool {
    let joined: String = text.split_whitespace().collect();
    let opens: String = opens.split_whitespace().collect();
    let mut rest = joined.as_str();
    while let Some(at) = rest.find(&opens) {
        rest = &rest[at + 1..];
        let statement = rest.split([';', '}']).next().unwrap_or("");
        if holds.iter().all(|held| statement.contains(held)) {
            return true;
        }
    }
    false
}

/// Whether a statement of `text` clears a child's environment
/// (`env_clear()`) and, by its `;`, sets no `XDG_RUNTIME_DIR`: the child
/// would keep its sockets in the machine's runtime folder, shared by
/// every test and by the Link the account runs.
pub fn clears_without_runtime(text: &str) -> bool {
    let joined: String = text.split_whitespace().collect();
    joined.match_indices(".env_clear()").any(|(at, _)| {
        !joined[at..]
            .split(';')
            .next()
            .unwrap_or("")
            .contains("XDG_RUNTIME_DIR")
    })
}

/// Whether a `loop` or `while` in `text` sleeps in its own block: a wait
/// made by hand, which the kit's waits replace.
pub fn polls(text: &str) -> bool {
    let joined: String = text.split_whitespace().collect();
    let bytes = joined.as_bytes();
    let word =
        |at: usize| at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
    let starts = joined
        .match_indices("loop{")
        .chain(joined.match_indices("while"))
        .map(|(at, _)| at)
        .filter(|&at| word(at));
    for at in starts {
        let Some(open) = joined[at..].find('{').map(|o| at + o) else {
            continue;
        };
        let mut depth = 0;
        let close = joined[open..]
            .char_indices()
            .find_map(|(i, c)| {
                depth += match c {
                    '{' => 1,
                    '}' => -1,
                    _ => 0,
                };
                (depth == 0).then_some(open + i)
            })
            .unwrap_or(joined.len());
        let block = &joined[open..close];
        if block.contains("thread::sleep(") || block.contains("time::sleep(") {
            return true;
        }
    }
    false
}

/// Whether an assertion in `text` reads `elapsed()`.
pub fn asserts_elapsed(text: &str) -> bool {
    statement_holds(text, "assert", &["elapsed()"])
}

/// Whether `text` finds a port free by binding `127.0.0.1:0`, reading its
/// port and letting the listener go, in one expression.
pub fn probes(text: &str) -> bool {
    statement_holds(text, "bind(\"127.0.0.1:0\")", &[".local_addr()", ".port()"])
}

/// The line from which `file`'s code is a test's: its first if it's a
/// test's file (in a `tests` folder, or `tests.rs`), else its first
/// `#[cfg(test)]`, else none.
fn tests_from(file: &Path, lines: &[&str]) -> Option<usize> {
    let a_tests_file = file.components().any(|part| part.as_os_str() == "tests")
        || file.file_name().is_some_and(|name| name == "tests.rs");
    if a_tests_file {
        return Some(0);
    }
    lines.iter().position(|line| {
        line.trim_start().starts_with("#[cfg(test)]")
            || line.trim_start().starts_with("#[cfg(all(test")
    })
}

/// What `file`, a path from its workspace's root, holds of each rule,
/// by the rule's name, each with its line; and its kit promises.
pub fn read(file: &Path, text: &str) -> (BTreeMap<&'static str, Vec<usize>>, usize) {
    let lines: Vec<&str> = text.lines().collect();
    let tests = tests_from(file, &lines);
    let mut found: BTreeMap<&'static str, Vec<usize>> = BTreeMap::new();
    for rule in RULES {
        let from = match rule.scope {
            Scope::All => 0,
            Scope::Tests => match tests {
                Some(from) => from,
                None => continue,
            },
        };
        let code = |at: usize| {
            lines
                .get(at)
                .map_or("", |l| l.split("//").next().unwrap_or(""))
        };
        let hits: Vec<usize> = match rule.pattern {
            Pattern::Whole(found) => {
                match found(&(from..lines.len()).map(code).collect::<Vec<_>>().join("\n")) {
                    true => vec![from + 1],
                    false => vec![],
                }
            }
            Pattern::Line(found) => (from..lines.len())
                .filter(|&at| found(code(at)))
                .map(|at| at + 1)
                .collect(),
            Pattern::Lines(found) => (from..lines.len())
                .filter(|&at| found(code(at), code(at + 1)))
                .map(|at| at + 1)
                .collect(),
        };
        if !hits.is_empty() {
            found.insert(rule.name, hits);
        }
    }
    (found, text.matches(PROMISE).count())
}

/// A skip printed, then returned from: on the line, or the next one.
fn printed_skip(code: &str, next: &str) -> bool {
    let printed = code.contains("eprintln!(\"skip") || code.contains("println!(\"skip");
    printed && (code.contains("return") || next.trim_start().starts_with("return"))
}

/// Every Rust file under `dirs` of `root`, but those under a path `skip`
/// names.
pub fn files(root: &Path, dirs: &[&str], skip: &[&str]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in dirs {
        walk(&root.join(dir), &mut found);
    }
    found.retain(|file| !skip.iter().any(|s| file.starts_with(root.join(s))));
    found.sort();
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            if !["target", "node_modules", ".git"].contains(&name.as_ref()) {
                walk(&path, found);
            }
        } else if name.ends_with(".rs") {
            found.push(path);
        }
    }
}

/// What a workspace's `test-kit.toml` keeps: the files each rule still
/// lists, and how many kit promises are left.
#[derive(Default)]
pub struct Listed {
    pub files: BTreeMap<String, Vec<String>>,
    pub promises: usize,
}

impl Listed {
    /// From `test-kit.toml`'s text.
    pub fn parse(text: &str) -> Listed {
        let table: toml::Table = toml::from_str(text).expect("test-kit.toml");
        let files = RULES
            .iter()
            .map(|rule| {
                let listed = table
                    .get(rule.name)
                    .and_then(|r| r.get("files"))
                    .and_then(|f| f.as_array())
                    .map(|f| {
                        f.iter()
                            .filter_map(|f| f.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                (rule.name.to_string(), listed)
            })
            .collect();
        let promises = table
            .get("promises")
            .and_then(|p| p.get("left"))
            .and_then(|n| n.as_integer())
            .unwrap_or(0) as usize;
        Listed { files, promises }
    }
}

/// A test program's kind, as its root declares it
/// ([`link_test_kit::program!`]): each test side by side in one process,
/// or each in a process of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Unit,
    System,
}

/// What a test program's root holds to declare each kind.
pub const DECLARED: [(&str, Kind); 2] = [
    ("link_test_kit::program!(unit);", Kind::Unit),
    ("link_test_kit::program!(system);", Kind::System),
];

/// A program Cargo builds for `cargo test`: a library's, a program's or a
/// test's, unless it says `test = false`.
#[derive(Debug)]
pub struct Program {
    /// Its root file, from the workspace's root.
    pub root: PathBuf,
    /// Its kind, or none where its root declares none.
    pub kind: Option<Kind>,
}

/// Each test program of the workspace `metadata` describes (`cargo
/// metadata --no-deps`), its root shown under `root`.
pub fn programs(root: &Path, metadata: &serde_json::Value) -> Vec<Program> {
    let mut found = Vec::new();
    let packages = metadata["packages"].as_array().into_iter().flatten();
    for package in packages {
        let kit = package["name"] == "link-test-kit";
        for target in package["targets"].as_array().into_iter().flatten() {
            let kinds = target["kind"].as_array().into_iter().flatten();
            let tested = target["test"].as_bool().unwrap_or(true)
                && kinds
                    .filter_map(|kind| kind.as_str())
                    .any(|kind| ["lib", "rlib", "bin", "test", "proc-macro"].contains(&kind));
            let Some(path) = target["src_path"].as_str().filter(|_| tested) else {
                continue;
            };
            let text = std::fs::read_to_string(path).unwrap_or_default();
            let kind = DECLARED.iter().find_map(|(declared, kind)| {
                let own = declared.trim_start_matches("link_test_kit::");
                (text.contains(declared) || (kit && text.contains(&format!("\n{own}"))))
                    .then_some(*kind)
            });
            let path = Path::new(path);
            found.push(Program {
                root: path.strip_prefix(root).unwrap_or(path).to_path_buf(),
                kind,
            });
        }
    }
    found
}

/// Each of `programs` whose root declares no kind. Without one, a test
/// finds Link's folders in the account's own home, the real `~/.lnk` and
/// `~/Link`, and says nothing of whether it may start a program.
pub fn undeclared(programs: &[Program]) -> Vec<String> {
    programs
        .iter()
        .filter(|program| program.kind.is_none())
        .map(|program| {
            format!(
                "{}: a test program that declares no kind: hold `#[cfg(test)] link_test_kit::program!(unit);` at its root, or `program!(system)` where its tests start programs",
                program.root.display()
            )
        })
        .collect()
}

/// Whether a line of a test's code starts a program or opens a socket: a
/// system test's work, which a unit program's tests, threads of one
/// process, would share.
pub fn starts(code: &str) -> bool {
    [
        "env::command(",
        "env::clean(",
        "ChildGroup::spawn",
        "Command::new(",
        "process::rerun(",
        "cargo()",
        ".spawn()",
        "TcpListener::bind",
        "UnixListener::bind",
        "Held::loopback",
        "HeldNumber::",
        "release::serve(",
    ]
    .iter()
    .any(|call| code.contains(call))
}

/// The files of `program`, under `root`: every Rust file beside its root
/// and below, for a root that is a crate's or a test program's `lib.rs`
/// or `main.rs` (but a `main.rs` beside a `lib.rs`), else its root and the
/// folder of its name.
fn program_files(root: &Path, program: &Program) -> Vec<PathBuf> {
    let file = root.join(&program.root);
    let mut found = Vec::new();
    let whole = file
        .file_name()
        .is_some_and(|name| name == "lib.rs" || name == "main.rs");
    // A program's `main.rs` beside its crate's `lib.rs` holds only itself:
    // the folder's other files are the library's.
    let beside_lib = file.file_name().is_some_and(|name| name == "main.rs")
        && file.with_file_name("lib.rs").exists();
    match (whole && !beside_lib, file.parent()) {
        (true, Some(dir)) => walk(dir, &mut found),
        _ => {
            found.push(file.clone());
            walk(&file.with_extension(""), &mut found);
        }
    }
    found.sort();
    found
}

/// Each unit program of `programs` whose tests start a program or open a
/// socket ([`starts`]), naming the line.
pub fn unit_starting(root: &Path, programs: &[Program]) -> Vec<String> {
    let mut found = Vec::new();
    for program in programs.iter().filter(|p| p.kind == Some(Kind::Unit)) {
        let hit = program_files(root, program).into_iter().find_map(|file| {
            let shown = file.strip_prefix(root).unwrap_or(&file).to_path_buf();
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            let from = tests_from(&shown, &lines)?;
            (from..lines.len())
                .find(|&at| starts(lines[at].split("//").next().unwrap_or("")))
                .map(|at| format!("{}:{}", shown.display(), at + 1))
        });
        if let Some(hit) = hit {
            found.push(format!(
                "{}: a unit program whose tests start a program or open a socket ({hit}): declare it `link_test_kit::program!(system)`, so each test runs in a process of its own",
                program.root.display()
            ));
        }
    }
    found
}

/// The rules over a workspace: each file under `dirs` that breaks one
/// unlisted, each listed that no longer does, and the kit promises if
/// they're more than `listed` says, or fewer (lower the count).
pub fn check(root: &Path, dirs: &[&str], skip: &[&str], listed: &Listed) -> Vec<String> {
    let mut found = Vec::new();
    let mut breaking: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut promises = 0;
    for file in files(root, dirs, skip) {
        let shown = file.strip_prefix(root).unwrap_or(&file).to_path_buf();
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let (rules, promised) = read(&shown, &text);
        promises += promised;
        for (name, lines) in rules {
            let file = shown.display().to_string();
            let allowed = listed.files.get(name).is_some_and(|l| l.contains(&file));
            if !allowed {
                let instead = RULES.iter().find(|r| r.name == name).unwrap().instead;
                found.push(format!("{file}:{}: {instead} ({name})", lines[0]));
            }
            breaking.entry(name).or_default().push(file);
        }
    }
    for (name, files) in &listed.files {
        for file in files {
            if !breaking
                .get(name.as_str())
                .is_some_and(|b| b.contains(file))
            {
                found.push(format!(
                    "{file}: keeps to {name} now: take it off test-kit.toml's list"
                ));
            }
        }
    }
    match promises.cmp(&listed.promises) {
        std::cmp::Ordering::Greater => found.push(format!(
            "{promises} raw calls say they move onto the testing kit, past the {} test-kit.toml allows: use the kit's helper instead",
            listed.promises
        )),
        std::cmp::Ordering::Less => found.push(format!(
            "{promises} raw calls say they move onto the testing kit, fewer than the {} test-kit.toml says: lower its count to {promises}",
            listed.promises
        )),
        std::cmp::Ordering::Equal => {}
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(file: &str, text: &str) -> Vec<&'static str> {
        read(Path::new(file), text).0.into_keys().collect()
    }

    #[test]
    fn a_skip_printed_and_returned_is_refused_and_a_log_line_is_not() {
        let skip = "    return eprintln!(\"skipped: no sshd\");";
        assert_eq!(rules("src/a.rs", skip), ["printed-skips"]);
        let skip = "eprintln!(\"skipped: no Xvfb\");\n    return;";
        assert_eq!(rules("src/a.rs", skip), ["printed-skips"]);
        assert!(rules("src/a.rs", "eprintln!(\"skipped {id}: missed\");\n}").is_empty());
        assert!(rules("src/a.rs", "// eprintln!(\"skipped\"); return").is_empty());
    }

    #[test]
    fn only_the_kit_reads_a_require_variable() {
        let read = "std::env::var_os(\"LNK_REQUIRE_SANDBOX\")";
        assert_eq!(rules("src/a.rs", read), ["require-variables"]);
        assert!(rules("src/a.rs", "// unless `LNK_REQUIRE_SANDBOX` is set").is_empty());
    }

    #[test]
    fn a_port_found_free_and_let_go_is_a_probe_and_a_held_one_is_not() {
        assert!(probes(
            "TcpListener::bind(\"127.0.0.1:0\")\n.unwrap()\n.local_addr()\n.unwrap()\n.port()"
        ));
        assert!(probes("bind(\"127.0.0.1:0\")?.local_addr()?.port())"));
        assert!(!probes(
            "let held = TcpListener::bind(\"127.0.0.1:0\").unwrap();\nlet port = held.local_addr().unwrap().port();"
        ));
        // A block's last expression keeps its listener: the next function's
        // port() isn't this statement's.
        assert!(!probes(
            "TcpListener::bind(\"127.0.0.1:0\").map(Held)\n}\nfn port(&self) -> u16 { self.0.local_addr().unwrap().port() }"
        ));
    }

    #[test]
    fn a_test_rule_reads_only_a_tests_code() {
        let code = "fn run() { std::env::temp_dir(); }";
        assert!(rules("src/a.rs", code).is_empty());
        assert_eq!(rules("tests/a.rs", code), ["temp-folders"]);
        let module = format!("{code}\n#[cfg(test)]\nmod tests {{ {code} }}");
        assert_eq!(rules("src/a.rs", &module), ["temp-folders"]);
        assert_eq!(read(Path::new("src/a.rs"), &module).0["temp-folders"], [3]);
    }

    #[test]
    fn each_test_pattern_is_named_by_its_rule() {
        for (text, rule) in [
            ("#[ignore]", "unexplained-ignores"),
            ("assert!(started.elapsed() < pause);", "elapsed-asserts"),
            (
                "assert!(\n    started.elapsed() < pause,\n    \"late\"\n);",
                "elapsed-asserts",
            ),
            (
                "unsafe { std::env::set_var(\"HOME\", dir) };",
                "process-environment",
            ),
            ("let path = \"/tmp/lnk-x\";", "tmp-paths"),
            (
                "while !ready() {\n    std::thread::sleep(Duration::from_millis(20));\n}",
                "poll-loops",
            ),
            (
                "loop {\n    if done() { break; }\n    tokio::time::sleep(STEP).await;\n}",
                "poll-loops",
            ),
            (
                "p.set_permissions(Permissions::from_mode(0o755));",
                "programs-by-hand",
            ),
            (
                "Command::new(lnk)\n    .env_clear()\n    .env(\"HOME\", &home)\n    .output();",
                "runtime-folders",
            ),
            (
                "std::fs::set_permissions(\n    &p,\n    Permissions::from_mode(0o755),\n)\n.unwrap();",
                "programs-by-hand",
            ),
        ] {
            assert_eq!(rules("tests/a.rs", text), [rule], "{text}");
        }
        assert!(rules("tests/a.rs", "#[ignore = \"needs a box\"]").is_empty());
        let own = "c.env_clear()\n    .env(\"HOME\", &home)\n    .env(\"XDG_RUNTIME_DIR\", made_runtime(&home));";
        assert!(rules("tests/a.rs", own).is_empty());
        // A loop that sleeps nowhere in its block, and a sleep outside one.
        let fine = "while next() {\n    seen += 1;\n}\nstd::thread::sleep(PAUSE);";
        assert!(rules("tests/a.rs", fine).is_empty());
        // A comment is no code: "while" in one starts no loop.
        assert!(
            rules(
                "tests/a.rs",
                "// waits while it runs\nfn f() { std::thread::sleep(P); }"
            )
            .is_empty()
        );
    }

    #[test]
    fn a_test_program_that_declares_no_kind_is_refused() {
        let root = link_test_kit::TempDir::new("undeclared");
        let given = root.join("given.rs");
        let undeclared = root.join("undeclared.rs");
        let untested = root.join("untested.rs");
        std::fs::write(&given, format!("#[cfg(test)]\n{}\n", DECLARED[0].0)).unwrap();
        for path in [&undeclared, &untested] {
            std::fs::write(path, "fn main() {}\n").unwrap();
        }
        let metadata = serde_json::json!({"packages": [{"name": "a", "targets": [
            {"kind": ["lib"], "src_path": given, "test": true},
            {"kind": ["test"], "src_path": undeclared, "test": true},
            {"kind": ["bin"], "src_path": untested, "test": false},
            {"kind": ["custom-build"], "src_path": untested, "test": false},
        ]}]});
        let programs = super::programs(root.path(), &metadata);
        assert_eq!(programs.len(), 2, "{programs:?}");
        let found = super::undeclared(&programs);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("undeclared.rs: "), "{found:?}");
        assert!(found[0].contains("program!(unit)"), "{found:?}");
    }

    #[test]
    fn a_unit_program_that_starts_a_program_must_be_a_system_one() {
        let root = link_test_kit::TempDir::new("unit-starting");
        std::fs::create_dir_all(root.join("tests/runs")).unwrap();
        std::fs::create_dir_all(root.join("tests/quiet")).unwrap();
        std::fs::write(
            root.join("tests/runs/main.rs"),
            "link_test_kit::program!(unit);\nmod start;\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tests/runs/start.rs"),
            "fn f() { link_test_kit::env::command(lnk, &home).output(); }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tests/quiet/main.rs"),
            "link_test_kit::program!(unit);\n// env::command( in a comment\n",
        )
        .unwrap();
        let program = |path: &str, kind| Program {
            root: PathBuf::from(path),
            kind: Some(kind),
        };
        let programs = [
            program("tests/runs/main.rs", Kind::Unit),
            program("tests/quiet/main.rs", Kind::Unit),
        ];
        let found = unit_starting(root.path(), &programs);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("tests/runs/start.rs:1"), "{found:?}");
        let system = [program("tests/runs/main.rs", Kind::System)];
        assert!(unit_starting(root.path(), &system).is_empty());
    }

    #[test]
    fn a_listed_file_that_keeps_the_rule_must_leave_the_list() {
        let root = link_test_kit::TempDir::new("test-kit-rule");
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("tests/a.rs"), "#[ignore = \"why\"]\n").unwrap();
        let mut listed = Listed::default();
        listed
            .files
            .insert("unexplained-ignores".into(), vec!["tests/a.rs".into()]);
        let found = check(root.path(), &["tests"], &[], &listed);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("take it off"), "{found:?}");
    }
}
