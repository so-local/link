//! Every file Link saves names its schema: it's written through
//! `link_files::saved`, which puts the `schema` line in, never as a bare
//! `toml::to_string` or `serde_json::to_vec` in a function that writes a
//! file, whether it's handed to the write at once, through a variable,
//! split across lines, to a `write_all` on a file the function opens, or
//! to another function of the file that writes it. `toml::ser::to_string`
//! and a JSON value's `to_string()` are bare too. A value serialized only
//! to be digested, or sent on a stream or to a terminal, is never text in
//! a file.
//! `saved.toml` names the files whose format is another program's or
//! never outlives a run, which stay, and the files held until their owner
//! declares their schema, a list that only shrinks. Test code, a file's
//! test module among it (and only its test module), and examples are
//! left out.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

/// `saved.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    /// Files whose writes aren't Link's saved files, each with why: a
    /// third party's format, a run's own runtime file or a build's output.
    pub allowed: BTreeMap<String, String>,
    /// Files that still write a saved file without its schema, each with
    /// why, until their owner declares it: the list only shrinks.
    #[serde(default)]
    pub held: BTreeMap<String, String>,
}

/// Whether a line of Rust writes a file whose text is a bare TOML or
/// JSON value, so no schema names it.
pub fn refused(line: &str) -> bool {
    let line = line.trim();
    if line.starts_with("//") {
        return false;
    }
    let writes = line.contains("write_private(") || line.contains("fs::write(");
    writes && bare(line)
}

/// Whether a line of Rust serializes a value to bare TOML or JSON text,
/// which names no schema. A `serde_json::to_value` is a value, not text,
/// one handed to a `sha256` is a digest's input, and one a stream or a
/// terminal is given is a message, not a file ([`streamed`]).
fn bare(line: &str) -> bool {
    !line.contains("sha256(")
        && !streamed(line)
        && (line.contains("toml::to_string")
            || line.contains("toml::ser::to_string")
            || ["to_string", "to_vec", "to_writer"]
                .iter()
                .any(|to| line.contains(&format!("serde_json::{to}")))
            || (line.contains(".to_string()")
                && (line.contains("json!(") || line.contains("serde_json::to_value("))))
}

/// Whether a line of Rust hands what it serializes to a stream or a
/// terminal: a message on a socket, a program's output, never a file.
fn streamed(line: &str) -> bool {
    let line: String = line.split_whitespace().collect();
    [
        "println!(",
        "print!(",
        "eprintln!(",
        "eprint!(",
        "writeln!(",
        ".send(",
    ]
    .iter()
    .any(|to| line.contains(to))
        || ["stdout", "stderr", "stdin", "stream", "socket"]
            .iter()
            .any(|to| {
                line.contains(&format!("to_writer(&mut{to}"))
                    || line.contains(&format!("to_writer({to}"))
                    || line.contains(&format!("{to}.write_all("))
            })
}

/// The names a function's lines bind to a JSON or TOML value, whose
/// `to_string()` is bare text: `let name = serde_json::json!(..)`, one
/// from `serde_json::to_value`, or one typed a `serde_json::Value`, a
/// `toml::Value` or a `toml::Table`.
fn values(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("let ")?;
            let rest = rest.strip_prefix("mut ").unwrap_or(rest);
            let (binding, value) = rest.split_once('=')?;
            let (name, typed) = binding.split_once(':').unwrap_or((binding, ""));
            let name = name.trim();
            let valued = [
                "json!(",
                "serde_json::to_value(",
                "toml::Table::",
                "toml::Value::",
            ]
            .iter()
            .any(|made| value.contains(made))
                || ["serde_json::Value", "toml::Value", "toml::Table"]
                    .iter()
                    .any(|kind| typed.trim() == *kind);
            (valued && !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
                .then(|| name.to_string())
        })
        .collect()
}

/// Whether a line of Rust opens a function: `fn`, after any of `pub`,
/// `pub(..)`, `async`, `const` and `unsafe`.
fn opens_fn(line: &str) -> bool {
    line.split_whitespace()
        .find(|word| {
            !matches!(*word, "pub" | "async" | "const" | "unsafe") && !word.starts_with("pub(")
        })
        .is_some_and(|word| word == "fn")
}

/// Whether a function's lines write a file: hand bytes to
/// `write_private` or `fs::write`, open one to write or append to
/// (`OpenOptions`, `File::create`, a `create_new`), or call an `append`
/// that isn't a method (a `Vec`'s) or the function's own name. A
/// `write_all` alone writes to whatever it's given, a stream among them.
fn writes_file(lines: &[&str]) -> bool {
    lines.iter().any(|line| {
        let line: String = line.split_whitespace().collect();
        !line.starts_with("//")
            && ([
                "write_private(",
                "fs::write(",
                "OpenOptions",
                "File::options(",
                "File::create(",
                "create_new(",
            ]
            .iter()
            .any(|opens| line.contains(opens))
                || line.match_indices("append(").any(|(at, _)| {
                    let before = &line[..at];
                    !before.ends_with(['.', '_']) && !before.ends_with("fn")
                }))
    })
}

/// The lines of `text` but a test module's: from a `#[cfg(test)]` whose
/// item is a `mod`, through its closing brace. A `#[cfg(test)]` on any
/// other item, a function or an `impl`, leaves it in.
pub(crate) fn outside_tests(text: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let item = lines[i + 1..]
            .iter()
            .map(|line| line.trim())
            .find(|line| !line.is_empty() && !line.starts_with("#["));
        let module = lines[i].trim() == "#[cfg(test)]"
            && item.is_some_and(|line| {
                line.split_whitespace()
                    .find(|word| *word != "pub" && !word.starts_with("pub("))
                    == Some("mod")
            });
        if !module {
            kept.push((i + 1, lines[i]));
            i += 1;
            continue;
        }
        let mut depth = 0i64;
        let mut opened = false;
        let mut within = Within::Code;
        while i < lines.len() {
            for c in code_of(lines[i], &mut within) {
                match c {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            let line = lines[i].trim();
            i += 1;
            if (opened && depth <= 0) || (!opened && line.ends_with(';') && line.contains("mod ")) {
                break;
            }
        }
    }
    kept
}

/// Where a scan of Rust source is at a line's end: in code, or within a
/// string or a raw string (`r#"`, its count of `#`), which run on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Within {
    Code,
    Text,
    Raw(usize),
}

/// The characters of `line` that are code, not a string, a character or
/// a comment, from where the scan is (`within`), left where it ends.
fn code_of(line: &str, within: &mut Within) -> Vec<char> {
    let chars: Vec<char> = line.chars().collect();
    let mut code = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match *within {
            Within::Text => {
                match chars[i] {
                    '\\' => i += 1,
                    '"' => *within = Within::Code,
                    _ => {}
                }
                i += 1;
            }
            Within::Raw(hashes) => {
                if chars[i] == '"'
                    && chars[i + 1..].iter().take_while(|c| **c == '#').count() >= hashes
                {
                    *within = Within::Code;
                    i += hashes;
                }
                i += 1;
            }
            Within::Code => {
                let c = chars[i];
                let next = chars.get(i + 1).copied();
                if c == '/' && next == Some('/') {
                    break;
                }
                let word_before = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
                if c == 'r' && !word_before && matches!(next, Some('"' | '#')) {
                    let hashes = chars[i + 1..].iter().take_while(|c| **c == '#').count();
                    if chars.get(i + 1 + hashes) == Some(&'"') {
                        *within = Within::Raw(hashes);
                        i += 2 + hashes;
                        continue;
                    }
                }
                if c == '"' {
                    *within = Within::Text;
                } else if c == '\'' {
                    // A character, `'{'` or `'\''`, never a lifetime.
                    let end = match next {
                        Some('\\') => chars[i + 2..]
                            .iter()
                            .position(|c| *c == '\'')
                            .map(|at| i + 2 + at),
                        Some(_) if chars.get(i + 2) == Some(&'\'') => Some(i + 2),
                        _ => None,
                    };
                    match end {
                        Some(end) => i = end,
                        None => code.push(c),
                    }
                } else {
                    code.push(c);
                }
                i += 1;
            }
        }
    }
    code
}

/// The lines, numbered from 1, of Rust source `text` that write a saved
/// file bare: a TOML or JSON value serialized bare in a function that
/// writes a file ([`writes_file`]), whether it's handed to the write at
/// once, through a variable or split across lines. A test module is left
/// out, and only a test module.
pub fn bare_writes(text: &str) -> Vec<usize> {
    let lines = outside_tests(text);
    let mut functions = Vec::new();
    let mut start = 0;
    for end in (1..=lines.len()).filter(|&i| i == lines.len() || opens_fn(lines[i].1)) {
        functions.push(&lines[start..end]);
        start = end;
    }
    let code = |function: &[(usize, &str)]| -> Vec<String> {
        function.iter().map(|(_, line)| line.to_string()).collect()
    };
    // A function that writes a file, or hands what it's given to one in
    // this file that does, however many calls away.
    let direct: Vec<bool> = functions
        .iter()
        .map(|function| {
            let lines = code(function);
            writes_file(&lines.iter().map(String::as_str).collect::<Vec<_>>())
        })
        .collect();
    let mut writing = direct.clone();
    loop {
        let writers = writers(&functions, &writing);
        let mut more = false;
        for (function, writes) in functions.iter().zip(writing.iter_mut()) {
            let calls = function.iter().skip(1).any(|(_, line)| {
                !line.trim().starts_with("//") && writers.iter().any(|name| calls(line, name))
            });
            if calls && !*writes {
                *writes = true;
                more = true;
            }
        }
        if !more {
            break;
        }
    }
    let writers = writers(&functions, &writing);
    let mut found = BTreeSet::new();
    for (function, writes) in functions.iter().zip(&direct) {
        let lines = code(function);
        let values = values(&lines.iter().map(String::as_str).collect::<Vec<_>>());
        let valued = |line: &str| {
            !streamed(line)
                && values
                    .iter()
                    .any(|value| line.contains(&format!("{value}.to_string()")))
        };
        let serialized = |line: &str| bare(line) || valued(line);
        // Where a function only calls a writer, what it serializes counts
        // when it's handed to that call, at once or through a name.
        let to_writer = |line: &str| writers.iter().any(|name| calls(line, name));
        for (number, line) in function.iter() {
            if line.trim().starts_with("//") {
                continue;
            }
            let through = !*writes
                && ((to_writer(line) && serialized(line))
                    || (serialized(line)
                        && bound(line).is_some_and(|name| {
                            function
                                .iter()
                                .any(|(_, later)| to_writer(later) && mentions(later, &name))
                        })));
            if refused(line) || (*writes && serialized(line)) || through {
                found.insert(*number);
            }
        }
    }
    found.into_iter().collect()
}

/// The names of the functions of `functions` that write, by `writing`.
fn writers(functions: &[&[(usize, &str)]], writing: &[bool]) -> Vec<String> {
    functions
        .iter()
        .zip(writing)
        .filter(|(_, writes)| **writes)
        .filter_map(|(function, _)| name_of(function.first()?.1))
        .collect()
}

/// The name a line binds with `let`, if it does.
fn bound(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("let ")?;
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Whether a line of Rust names `name` as a whole word.
fn mentions(line: &str, name: &str) -> bool {
    line.match_indices(name).any(|(at, _)| {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        !line[..at].chars().next_back().is_some_and(word)
            && !line[at + name.len()..].chars().next().is_some_and(word)
    })
}

/// The name of the function a line opens ([`opens_fn`]).
fn name_of(line: &str) -> Option<String> {
    let rest = line.split_once("fn ")?.1;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Whether a line of Rust calls the function `name` of its own file: by
/// itself, as `self.` or `Self::` names it, never another value's method
/// of that name or a longer name ending in it.
fn calls(line: &str, name: &str) -> bool {
    line.match_indices(&format!("{name}(")).any(|(at, _)| {
        let before = &line[..at];
        let free = !before
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':');
        let own = before.ends_with("self.") || before.ends_with("Self::");
        (free || own) && !before.trim_end().ends_with("fn")
    })
}

/// Each problem with the Rust under `root`, by the rules in `rules` (a
/// `saved.toml`): none when every saved file names its schema, or its
/// writer is allowed or held by name, and every held file still writes
/// one bare.
pub fn problems(root: &Path, rules: &Path) -> Vec<String> {
    let rules: Rules = match std::fs::read_to_string(rules)
        .map_err(|err| err.to_string())
        .and_then(|text| toml::from_str(&text).map_err(|err| err.to_string()))
    {
        Ok(rules) => rules,
        Err(err) => return vec![format!("{} isn't read: {err}", rules.display())],
    };
    let mut found = Vec::new();
    let mut writing = BTreeSet::new();
    for file in crate::shapes::sources(root) {
        if crate::shapes::test_code(&file) || rules.allowed.contains_key(&file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&file)) else {
            continue;
        };
        let lines = bare_writes(&text);
        if lines.is_empty() {
            continue;
        }
        writing.insert(file.clone());
        if rules.held.contains_key(&file) {
            continue;
        }
        found.extend(lines.into_iter().map(|line| {
            format!("{file}:{line}: a saved file written without its schema: write it with link_files::saved")
        }));
    }
    for file in rules.held.keys().filter(|file| !writing.contains(*file)) {
        found.push(format!(
            "{file} writes no saved file bare now: take it out of saved.toml's held"
        ));
    }
    for file in rules
        .allowed
        .keys()
        .filter(|file| !root.join(file).is_file())
    {
        found.push(format!(
            "{file} is gone: take it out of saved.toml's allowed"
        ));
    }
    found
}
