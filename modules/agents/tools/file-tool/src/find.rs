//! `glob` and `grep`: files by a glob of their name, and lines by a
//! regular expression, under a folder of the agent's, as Claude Code's
//! Glob and Grep find them.

use super::*;

/// `glob` and `grep`: under `folder`, the files `query` names, and for a
/// `grep`, their lines it matches, each path as the model gives it.
/// Walked by handles, each folder opened from the one before and each
/// file read on the handle it was opened as: a link is never followed,
/// one swapped in meanwhile included. A folder past
/// [`link_files::nofollow::DEEPEST`] below `folder` is passed over: a
/// handle is held for each folder on the way down. What a folder's
/// `.gitignore` names is passed over as git does. Its
/// answer counts what it passed over (`skipped`: hidden, ignored, large,
/// binary or too deep), and where it stopped early, why (`stopped`: the
/// most found, the most files and folders looked at, or its time, past
/// `time_max`: `find_time_max_ms`, [`Limits`]).
pub(crate) fn find(
    folder: &Place,
    query: &Query,
    time_max: std::time::Duration,
) -> anyhow::Result<Value> {
    let top = folder
        .folder()
        .map_err(|_| anyhow!("there's no such folder in your files folder"))?;
    let started = std::time::Instant::now();
    let mut found = Vec::new();
    let mut looked = 0;
    let mut skipped = Skipped::default();
    let mut stopped = None;
    let prefix = match folder.parts.is_empty() {
        true => folder.shown.clone(),
        false => format!("{}/", folder.shown(folder.parts.len())),
    };
    // A path below `folder` is what follows it.
    let below = prefix.len();
    // Depth first, each folder's files before its folders, which are
    // walked in order: a handle held for each folder on the way down,
    // with its `.gitignore`.
    let ignore = Ignore::of(&top);
    let mut walk: Vec<Walked> = vec![Walked {
        dir: top,
        prefix,
        folders: Vec::new(),
        ignore,
    }];
    let mut fresh = true;
    'walk: loop {
        let depth = walk.len();
        let Some(at) = walk.last() else {
            break;
        };
        if fresh {
            fresh = false;
            let mut names = at.dir.names().unwrap_or_default();
            names.reverse();
            let mut folders = Vec::new();
            for entry in names {
                if started.elapsed() >= time_max {
                    stopped = Some("time");
                    break 'walk;
                }
                let file_name = entry.to_string_lossy().into_owned();
                let Ok(kind) = at.dir.kind(&entry) else {
                    continue;
                };
                // Links: never.
                if kind == Kind::Link {
                    continue;
                }
                if file_name.starts_with('.') {
                    skipped.hidden += 1;
                    continue;
                }
                let relative = format!("{}{file_name}", at.prefix);
                let is_folder = kind == Kind::Folder;
                if ignored(&walk, &relative, is_folder) {
                    skipped.ignored += 1;
                    continue;
                }
                looked += 1;
                if looked > limits().files_max {
                    stopped = Some("looked");
                    break 'walk;
                }
                if is_folder {
                    folders.push(entry);
                    continue;
                }
                if !query.names(&relative[below..]) {
                    continue;
                }
                if !query.reads() {
                    found.push(json!({"path": relative}));
                    if found.len() >= limits().found_max {
                        stopped = Some("found");
                        break 'walk;
                    }
                    continue;
                }
                // A plain file only, and small: a pipe would hold the
                // tool for good.
                let Ok(mut opened) = at.dir.open_file(&entry, libc::O_RDONLY, 0) else {
                    continue;
                };
                if opened
                    .metadata()
                    .map_or(true, |m| m.len() > limits().searched_max)
                {
                    skipped.large += 1;
                    continue;
                }
                let mut bytes = Vec::new();
                if (&mut opened)
                    .take(limits().searched_max)
                    .read_to_end(&mut bytes)
                    .is_err()
                {
                    continue;
                }
                let Ok(content) = String::from_utf8(bytes) else {
                    skipped.binary += 1;
                    continue;
                };
                if query.found_in(&relative, &content, &mut found) {
                    stopped = Some("found");
                    break 'walk;
                }
            }
            if let Some(at) = walk.last_mut() {
                at.folders = folders;
            }
        }
        let Some(at) = walk.last_mut() else {
            break;
        };
        // The next folder down, the first by name; none left: back up.
        match at.folders.pop() {
            Some(_) if depth > link_files::nofollow::DEEPEST => {
                skipped.deep += 1 + at.folders.len();
                at.folders.clear();
            }
            Some(next) => {
                let shown = format!("{}{}/", at.prefix, next.to_string_lossy());
                if let Ok(next) = at.dir.folder(&next) {
                    let ignore = Ignore::of(&next);
                    walk.push(Walked {
                        dir: next,
                        prefix: shown,
                        folders: Vec::new(),
                        ignore,
                    });
                    fresh = true;
                }
            }
            None => {
                walk.pop();
            }
        }
    }
    let mut answer = json!({"found": found, "more": stopped.is_some() || skipped.deep > 0});
    skipped.said(&mut answer);
    if let Some(stopped) = stopped {
        answer["stopped"] = stopped.into();
    }
    Ok(answer)
}

/// A folder on the way down: its handle, its path as the model
/// gives it (with a `/` after), the folders in it still to walk, and its
/// `.gitignore`'s rules.
struct Walked {
    dir: Dir,
    prefix: String,
    folders: Vec<std::ffi::OsString>,
    ignore: Option<Ignore>,
}

/// What a `glob` or `grep` passed over, by why: the same on disk and through the
/// bridge, but `hidden` and `ignored`, which a bridged Home never holds.
#[derive(Debug, Default)]
pub(crate) struct Skipped {
    pub(crate) hidden: usize,
    pub(crate) ignored: usize,
    pub(crate) large: usize,
    pub(crate) binary: usize,
    pub(crate) deep: usize,
}

impl Skipped {
    /// `answer` with `skipped`, each count above none, if any is.
    pub(crate) fn said(&self, answer: &mut Value) {
        let counts = [
            ("hidden", self.hidden),
            ("ignored", self.ignored),
            ("large", self.large),
            ("binary", self.binary),
            ("deep", self.deep),
        ];
        let said: serde_json::Map<String, Value> = counts
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(why, n)| (why.to_string(), (*n).into()))
            .collect();
        if !said.is_empty() {
            answer["skipped"] = said.into();
        }
    }
}

/// Whether `relative` (a path as the model gives it, a folder's where
/// `is_folder`) is what a `.gitignore` on the way down names: the
/// deepest one with a rule for it says, as git has it.
fn ignored(walk: &[Walked], relative: &str, is_folder: bool) -> bool {
    walk.iter()
        .rev()
        .find_map(|at| {
            let ignore = at.ignore.as_ref()?;
            ignore.says(relative.strip_prefix(&at.prefix)?, is_folder)
        })
        .unwrap_or(false)
}

/// The name of the file whose rules `glob` and `grep` keep to in each
/// folder.
const GITIGNORE: &str = ".gitignore";

/// A folder's `.gitignore`: its rules, each a glob of a name, or of a
/// path below the folder when it holds a `/`; `!` before one includes
/// again what an earlier rule named, and a `/` after one names only
/// folders. The last rule naming a path says.
struct Ignore {
    rules: Vec<Rule>,
}

struct Rule {
    pattern: regex::Regex,
    by_path: bool,
    negated: bool,
    folders_only: bool,
}

impl Ignore {
    /// The `.gitignore` in `dir`, opened on its handle, never through a
    /// link: none when it has none, or it's larger than
    /// `ignore_file_max` ([`crate::Limits`]) or isn't text.
    fn of(dir: &Dir) -> Option<Self> {
        let mut opened = dir
            .open_file(OsStr::new(GITIGNORE), libc::O_RDONLY, 0)
            .ok()?;
        if opened.metadata().ok()?.len() > limits().ignore_file_max {
            return None;
        }
        let mut text = String::new();
        (&mut opened)
            .take(limits().ignore_file_max)
            .read_to_string(&mut text)
            .ok()?;
        Some(Self::parse(&text))
    }

    fn parse(text: &str) -> Self {
        let rules = text
            .lines()
            .filter_map(|line| {
                let line = line.trim_end();
                if line.is_empty() || line.starts_with('#') {
                    return None;
                }
                let (negated, line) = match line.strip_prefix('!') {
                    Some(rest) => (true, rest),
                    None => (false, line.strip_prefix('\\').unwrap_or(line)),
                };
                let (folders_only, line) = match line.strip_suffix('/') {
                    Some(rest) => (true, rest),
                    None => (false, line),
                };
                let by_path = line.contains('/');
                let line = line.strip_prefix('/').unwrap_or(line);
                let pattern = pattern(&glob(line), false).ok()?;
                Some(Rule {
                    pattern,
                    by_path,
                    negated,
                    folders_only,
                })
            })
            .collect();
        Self { rules }
    }

    /// Whether its rules ignore `below` (a path below its folder, a
    /// folder's where `is_folder`): none when no rule names it.
    fn says(&self, below: &str, is_folder: bool) -> Option<bool> {
        let name = below.rsplit('/').next().unwrap_or(below);
        self.rules
            .iter()
            .rev()
            .find(|rule| {
                (is_folder || !rule.folders_only)
                    && rule.pattern.is_match(match rule.by_path {
                        true => below,
                        false => name,
                    })
            })
            .map(|rule| !rule.negated)
    }
}

/// What a `glob` or `grep` looks for, built once from its arguments,
/// the same on disk and through the bridge: a glob of a file's name, or
/// of its path below the folder searched when it holds `/`, any case (a
/// `glob`'s `pattern`, a `grep`'s `glob`); and a regular expression a
/// line matches (a `grep`'s `pattern`).
pub(crate) struct Query {
    name: Option<regex::Regex>,
    by_path: bool,
    text: Option<regex::Regex>,
    mode: Mode,
}

/// What a `grep` gives of what it finds, its `output_mode`, as Claude
/// Code's Grep takes it: the files that hold a match, the default; each
/// line that matches; or how many lines match in each file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Files,
    Content,
    Count,
}

impl Mode {
    /// The mode `output_mode` names, the files left out.
    fn of(arguments: &Value) -> anyhow::Result<Self> {
        Ok(match arguments.get(OUTPUT_MODE).and_then(Value::as_str) {
            None | Some("files_with_matches") => Self::Files,
            Some("content") => Self::Content,
            Some("count") => Self::Count,
            Some(other) => {
                bail!("'{OUTPUT_MODE}' is files_with_matches, content or count, not {other:?}")
            }
        })
    }
}

/// The argument naming what a `grep` gives.
const OUTPUT_MODE: &str = "output_mode";

impl Query {
    /// Call `call`'s (`glob` or `grep`) query, from its `arguments`.
    pub(crate) fn of(call: &str, arguments: &Value) -> anyhow::Result<Self> {
        let given = |key: &str| {
            arguments
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        };
        let pattern_given =
            given("pattern").with_context(|| format!("{call} needs a 'pattern' to look for"))?;
        let ((name, name_key), text) = match call {
            "glob" => ((Some(pattern_given), "pattern"), None),
            "grep" => ((given("glob"), "glob"), Some(pattern_given)),
            other => bail!("no call {other:?}"),
        };
        let mode = match call {
            "grep" => Mode::of(arguments)?,
            _ => Mode::default(),
        };
        Ok(Self {
            mode,
            name: name
                .map(|name| {
                    pattern(&glob(name), true)
                        .map_err(|e| anyhow!("'{name_key}' isn't a glob: {e}"))
                })
                .transpose()?,
            by_path: name.is_some_and(|name| name.contains('/')),
            text: text
                .map(|text| {
                    pattern(text, false).map_err(|e| {
                        anyhow!(
                            "'pattern' isn't a regular expression: escape '.', '(', '[' and the \
                             like for literal text, as '\\('\n{e}"
                        )
                    })
                })
                .transpose()?,
        })
    }

    /// Whether the file at `path`, below the folder searched, is one it
    /// looks for: by its name, or by `path` for a glob holding `/`.
    pub(crate) fn names(&self, path: &str) -> bool {
        let Some(name) = &self.name else {
            return true;
        };
        match self.by_path {
            true => name.is_match(path),
            false => name.is_match(path.rsplit('/').next().unwrap_or(path)),
        }
    }

    /// Whether it looks in the files it names for lines.
    pub(crate) fn reads(&self) -> bool {
        self.text.is_some()
    }

    /// Whether `line` is one it looks for.
    pub(crate) fn line(&self, line: &str) -> bool {
        self.text.as_ref().is_some_and(|text| text.is_match(line))
    }

    /// `found` with what file `path`, `content`, holds, as its [`Mode`]
    /// gives it: the path, each line, or the count. Whether `found` then
    /// holds the most it gives (`found_max`, [`Limits`]).
    pub(crate) fn found_in(&self, path: &str, content: &str, found: &mut Vec<Value>) -> bool {
        let most = limits().found_max;
        match self.mode {
            Mode::Files => {
                if content.lines().any(|line| self.line(line)) {
                    found.push(json!({"path": path}));
                }
            }
            Mode::Count => {
                let count = content.lines().filter(|line| self.line(line)).count();
                if count > 0 {
                    found.push(json!({"path": path, "count": count}));
                }
            }
            Mode::Content => {
                for (n, line) in content.lines().enumerate() {
                    if found.len() >= most {
                        break;
                    }
                    if self.line(line) {
                        let line = &line[..line.floor_char_boundary(limits().line_max)];
                        found.push(json!({"path": path, "line": n + 1, "text": line}));
                    }
                }
            }
        }
        found.len() >= most
    }
}

/// A `glob`'s or `grep`'s answer as the model reads it, as Claude Code's
/// Glob and Grep give theirs: a path a line, `path:line:text` for each
/// line found, or `path:count`, or that `call` found none. A last line,
/// in brackets, says what was passed over and why it stopped early, where
/// it did.
pub(crate) fn listed(call: &str, answer: &Value) -> String {
    let found = answer["found"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut out: Vec<String> = found
        .iter()
        .map(|found| {
            let path = found["path"].as_str().unwrap_or_default();
            match (found.get("line"), found.get("count")) {
                (Some(line), _) => {
                    format!(
                        "{path}:{line}:{}",
                        found["text"].as_str().unwrap_or_default()
                    )
                }
                (None, Some(count)) => format!("{path}:{count}"),
                (None, None) => path.to_string(),
            }
        })
        .collect();
    if out.is_empty() {
        out.push(match call {
            "glob" => "No files found".into(),
            _ => "No matches found".into(),
        });
    }
    let mut notes = Vec::new();
    if let Some(skipped) = answer["skipped"].as_object() {
        let said: Vec<String> = skipped
            .iter()
            .map(|(why, count)| {
                let why = match why.as_str() {
                    "ignored" => "named by a .gitignore".into(),
                    "large" => format!("over {} MiB", limits().searched_max >> 20),
                    "binary" => "not text".into(),
                    "deep" => "folders too deep".into(),
                    other => other.to_string(),
                };
                format!("{count} {why}")
            })
            .collect();
        notes.push(format!("passed over {}", said.join(", ")));
    }
    let stopped = match answer["stopped"].as_str() {
        Some("found") => Some(format!(
            "stopped at the most it gives, {}",
            limits().found_max
        )),
        Some("looked") => Some(format!(
            "stopped at the most files and folders it looks at, {}",
            limits().files_max
        )),
        Some("time") => Some("stopped at its time; name a folder in 'path' to look in less".into()),
        _ => None,
    };
    notes.extend(stopped);
    if !notes.is_empty() {
        out.push(format!("[{}]", notes.join("; ")));
    }
    out.join("\n")
}

/// `pattern` as a regular expression, any case or not, compiled to at
/// most `pattern_max` ([`Limits`]).
fn pattern(pattern: &str, any_case: bool) -> Result<regex::Regex, regex::Error> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(any_case)
        .size_limit(limits().pattern_max)
        .dfa_size_limit(limits().pattern_max)
        .build()
}

/// Glob `glob` as a regular expression of a whole name or path: `*` any
/// characters but `/`, `?` one, `**/` any folders (none too), `**` any
/// characters, `[a-z]` one of a class and `[!a-z]` one not; the rest as
/// it is.
fn glob(glob: &str) -> String {
    let chars: Vec<char> = glob.chars().collect();
    let mut out = String::from("^");
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        i += 1;
        match c {
            '*' if chars.get(i) == Some(&'*') => {
                i += 1;
                match chars.get(i) == Some(&'/') {
                    true => {
                        i += 1;
                        out.push_str("(?:[^/]*/)*");
                    }
                    false => out.push_str(".*"),
                }
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '[' => match class(&chars[i..]) {
                Some((class, used)) => {
                    out.push_str(&class);
                    i += used;
                }
                // Never closed: a `[` itself.
                None => out.push_str(r"\["),
            },
            c => out.push_str(&regex::escape(c.encode_utf8(&mut [0; 4]))),
        }
    }
    out.push('$');
    out
}

/// The class `chars` starts, past its `[`, as a regular expression's,
/// and the characters it took: none if it's never closed. A `]` first is
/// in it, and a `-` first or last.
fn class(chars: &[char]) -> Option<(String, usize)> {
    let mut out = String::from("[");
    let mut i = 0;
    if chars.first() == Some(&'!') {
        out.push('^');
        i += 1;
    }
    let start = i;
    while let Some(&c) = chars.get(i) {
        i += 1;
        match c {
            ']' if i - 1 > start => {
                out.push(']');
                return Some((out, i));
            }
            '-' if i - 1 > start && chars.get(i).is_some_and(|n| *n != ']') => out.push('-'),
            c => out.push_str(&regex::escape(c.encode_utf8(&mut [0; 4]))),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A glob is a regular expression of a whole name or path, any case:
    /// its `*`, `?`, `**/` and classes as a glob means them, the rest as
    /// it is.
    #[test]
    fn a_glob_matches_whole_names_and_paths() {
        let matches = |g: &str, path: &str| pattern(&glob(g), true).unwrap().is_match(path);
        for (g, yes, no) in [
            (
                "*.rs",
                &["main.rs", "MAIN.RS", ".rs"][..],
                &["main.rs.bak", "src/main.rs"][..],
            ),
            ("?.txt", &["a.txt"], &["ab.txt", ".txt", "/.txt"]),
            (
                "src/**/test_*.py",
                &["src/test_a.py", "src/a/b/test_x.py", "SRC/Test_A.PY"],
                &["src/a/x.py", "lib/test_a.py", "src/test_a.pyc"],
            ),
            ("**/*.md", &["a.md", "x/y/a.md"], &["a.mdx"]),
            ("src/**", &["src/a", "src/a/b"], &["lib/a"]),
            ("[abc].md", &["b.md", "C.md"], &["d.md", "ab.md"]),
            ("[a-c]x", &["bx"], &["dx"]),
            ("[!a-c]x", &["dx"], &["ax", "x"]),
            ("[]x]", &["]", "x"], &["[]x]"]),
            ("[-a]", &["-", "a"], &["b"]),
            ("[a-]", &["-", "a"], &["b"]),
            ("a[b", &["a[b"], &["ab"]),
            ("a.b", &["a.b"], &["axb"]),
            ("(x)+{2}", &["(x)+{2}"], &["xx"]),
        ] {
            for path in yes {
                assert!(matches(g, path), "{g} matches {path}: {}", glob(g));
            }
            for path in no {
                assert!(!matches(g, path), "{g} doesn't match {path}: {}", glob(g));
            }
        }
        // A pattern too large compiled is refused, never built.
        let huge = Query::of("grep", &json!({"pattern": "(a|b|c|d){1000}{1000}"}));
        assert!(huge.is_err());
        let why = Query::of("grep", &json!({"pattern": "f("}))
            .err()
            .unwrap()
            .to_string();
        assert!(why.contains("escape"), "{why}");
        // A glob's pattern is a glob, never a regular expression: `f(`
        // is a name.
        let named = Query::of("glob", &json!({"pattern": "f("})).unwrap();
        assert!(named.names("f(") && !named.reads());
        // A grep's glob narrows the files it reads.
        let narrowed = Query::of("grep", &json!({"pattern": "x", "glob": "*.rs"})).unwrap();
        assert!(narrowed.names("src/a.rs") && !narrowed.names("a.md") && narrowed.reads());
        assert!(Query::of("grep", &json!({"glob": "*.rs"})).is_err());
    }

    /// `grep` and `glob` pass over what a `.gitignore` names, as git does, deeper
    /// rules over shallower ones, and says what it passed over and why.
    #[test]
    fn find_keeps_to_gitignore_and_says_what_it_skipped() {
        let dir = link_test_kit::TempDir::new("file-ignore");
        let home = dir.join("home");
        for folder in ["node_modules/pkg", "src/gen", "target", "docs"] {
            std::fs::create_dir_all(home.join(folder)).unwrap();
        }
        std::fs::write(
            home.join(".gitignore"),
            "# built\n/target/\nnode_modules/\n*.log\n!keep.log\n",
        )
        .unwrap();
        std::fs::write(home.join("src/.gitignore"), "gen/\n").unwrap();
        for file in [
            "node_modules/pkg/index.js",
            "target/out.rs",
            "src/gen/made.rs",
            "src/main.rs",
            "docs/a.log",
            "docs/keep.log",
            "docs/target",
        ] {
            std::fs::write(home.join(file), "needle\n").unwrap();
        }
        std::fs::write(home.join("docs/blob.bin"), b"needle\xff\n").unwrap();
        let files = Files {
            folder: link_agent_contract::folder::Folder {
                home: home.clone(),
                agent: dir.join("agent"),
            },
            write: vec![],
            teams: vec![],
        };
        let found = apply_call(&files, "grep", &json!({"pattern": "needle"})).unwrap();
        let mut paths: Vec<&str> = found["found"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap())
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            ["docs/keep.log", "docs/target", "src/main.rs"],
            "{found}"
        );
        assert_eq!(
            found["skipped"],
            json!({"hidden": 2, "ignored": 4, "binary": 1}),
            "{found}"
        );
        assert_eq!(found["more"], false, "{found}");
        // An empty folder is the files folder.
        let all = apply_call(&files, "glob", &json!({"path": "", "pattern": "main.rs"})).unwrap();
        assert_eq!(all["found"], json!([{"path": "src/main.rs"}]), "{all}");
    }

    /// `glob` holds a handle for each folder on the way down, so it goes
    /// no deeper than [`link_files::nofollow::DEEPEST`] below where it
    /// starts: past it, folders are passed over and `more` says so.
    #[test]
    fn find_goes_only_so_deep() {
        let dir = link_test_kit::TempDir::new("file-deep");
        let home = dir.join("home");
        let deepest = link_files::nofollow::DEEPEST;
        let mut at = home.join("work");
        for n in 1..=deepest + 5 {
            at.push(format!("d{n}"));
        }
        std::fs::create_dir_all(&at).unwrap();
        // One file at the deepest folder found, one past it not.
        let mut reached = home.join("work");
        for n in 1..=deepest {
            reached.push(format!("d{n}"));
        }
        std::fs::write(reached.join("reached.md"), "x").unwrap();
        std::fs::write(at.join("past.md"), "x").unwrap();
        let files = Files {
            folder: link_agent_contract::folder::Folder {
                home: home.clone(),
                agent: dir.join("agent"),
            },
            write: vec![],
            teams: vec![],
        };
        let found =
            apply_call(&files, "glob", &json!({"path": "work", "pattern": "*.md"})).unwrap();
        let paths: Vec<&str> = found["found"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths.len(), 1, "{found}");
        assert!(paths[0].ends_with("reached.md"), "{found}");
        assert_eq!(found["more"], true, "{found}");
        assert_eq!(found["skipped"]["deep"], 1, "{found}");
    }

    /// `grep` and `glob` stop at their time, `find_time_max_ms`,
    /// answering with what they found and saying why: given none, each
    /// stops at the first name; given its own, it looks through.
    #[test]
    fn find_stops_at_its_time() {
        let dir = link_test_kit::TempDir::new("file-time");
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("notes.md"), "needle\n").unwrap();
        let place = Place {
            root: home,
            shown: String::new(),
            parts: Vec::new(),
        };
        for (call, asked) in [
            ("grep", json!({"pattern": "needle"})),
            ("glob", json!({"pattern": "*.md"})),
        ] {
            let query = Query::of(call, &asked).unwrap();
            let stopped = find(&place, &query, std::time::Duration::ZERO).unwrap();
            assert_eq!(stopped["stopped"], "time", "{call}: {stopped}");
            assert_eq!(stopped["more"], true, "{call}: {stopped}");
            assert_eq!(stopped["found"], json!([]), "{call}: {stopped}");
            let through = find(&place, &query, limits().find_time_max()).unwrap();
            assert!(through.get("stopped").is_none(), "{call}: {through}");
            assert_eq!(through["found"][0]["path"], "notes.md", "{call}: {through}");
        }
    }
}
