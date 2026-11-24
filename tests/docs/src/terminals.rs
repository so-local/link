//! What a page's fenced blocks may hold: a terminal block is `lnk` and
//! `curl` commands alone, with no comment, and a file a reader writes is a
//! fenced block of its own, whose one comment is its path.

use std::path::Path;

use crate::site::is_page;
use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[
    Rule {
        id: "terminal-blocks",
        scope: Scope::Every,
        fix: "a terminal block holds lnk and curl commands alone, one a line, with no comment: \
              no other program, pipe, redirect, heredoc or continued line, but the install line \
              `curl -fsSL https://local.link/install.sh | sh`; show a file as its own fenced \
              block and say in prose where it lives",
        check: Check::Each(terminal_blocks),
    },
    Rule {
        id: "file-blocks",
        scope: Scope::Every,
        fix: "a file's fenced block holds no comment but its path, as its first line \
              (`# ~/Link/Safe/Agents/main/agent.toml`): say the rest in the prose around it",
        check: Check::Each(file_blocks),
    },
];

/// The one line a terminal block may hold with a pipe: the install, which
/// runs before `lnk` exists.
pub const INSTALL: &str = "curl -fsSL https://local.link/install.sh | sh";

/// The fences a reader types into a terminal.
const TERMINALS: [&str; 6] = ["sh", "bash", "shell", "zsh", "console", "terminal"];

/// The fences of files whose comments start with `#`.
const FILES: [&str; 8] = ["toml", "yaml", "yml", "ini", "env", "dotenv", "conf", "cfg"];

/// A fenced block's lines, each with its line number.
pub type Lines<'a> = Vec<(usize, &'a str)>;

/// Each fenced block of `text`: its language and its lines.
pub fn fences(text: &str) -> Vec<(&str, Lines<'_>)> {
    let mut found = Vec::new();
    let mut open: Option<(&str, &str, Lines<'_>)> = None;
    for (n, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let marker: String = trimmed
            .chars()
            .take_while(|&c| c == '`' || c == '~')
            .collect();
        let fence = marker.len() >= 3
            && (marker.chars().all(|c| c == '`') || marker.chars().all(|c| c == '~'));
        match open.take() {
            Some((close, language, lines)) => {
                if fence && trimmed.starts_with(close) && trimmed[close.len()..].trim().is_empty() {
                    found.push((language, lines));
                } else {
                    let mut lines = lines;
                    lines.push((n + 1, line));
                    open = Some((close, language, lines));
                }
            }
            None if fence => {
                let close = &trimmed[..marker.len()];
                let language = trimmed[marker.len()..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                open = Some((close, language, Vec::new()));
            }
            None => {}
        }
    }
    found
}

/// `line` with its quoted strings emptied, so what's quoted reads as one
/// word and nothing inside it as shell.
fn unquoted(line: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in line.chars() {
        match quote {
            Some(q) => {
                if escaped {
                    escaped = false;
                } else if c == '\\' && q == '"' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                    out.push(c);
                }
            }
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                out.push(c);
            }
            None => out.push(c),
        }
    }
    out
}

/// Why a terminal block's `line` isn't one `lnk` command, if it isn't.
pub fn not_lnk(line: &str) -> Option<&'static str> {
    let line = line.trim();
    if line.is_empty() || line == INSTALL {
        return None;
    }
    if line.starts_with('#') {
        return Some("a comment");
    }
    let program = line.split_whitespace().next().unwrap_or("");
    if program != "lnk" && program != "curl" {
        return Some("a command that isn't lnk or curl");
    }
    let bare = unquoted(line);
    if bare.trim_end().ends_with('\\') {
        return Some("a line continued with a backslash");
    }
    if bare.contains("$(") || bare.contains('`') {
        return Some("another command inside it");
    }
    for word in bare.split_whitespace() {
        if word.starts_with('#') {
            return Some("a comment");
        }
        if word.starts_with("<<") {
            return Some("a heredoc");
        }
        // A choice is words joined by a pipe with no space, `aws|gcp`; a
        // pipe standing alone runs another program.
        if matches!(word, "|" | "||" | "&&" | "&" | ";")
            || word.ends_with(';')
            || word.starts_with('|')
        {
            return Some("another command after it");
        }
        let redirect = word.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
        if redirect.starts_with('>') || word == "<" {
            return Some("a redirect");
        }
    }
    None
}

/// Whether `line` of a file's block holds a comment, outside its strings.
fn has_comment(line: &str) -> bool {
    unquoted(line).contains('#')
}

/// Whether `line` is a file block's path comment, `# <path>`.
fn path_comment(line: &str) -> bool {
    let Some(rest) = line.trim().strip_prefix("# ") else {
        return false;
    };
    let rest = rest.trim();
    !rest.is_empty()
        && !rest.contains(char::is_whitespace)
        && (rest.contains('/') || rest.contains('.'))
}

fn terminal_blocks(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !is_page(tree, file) {
        return Vec::new();
    }
    fences(text)
        .into_iter()
        .filter(|(language, _)| TERMINALS.contains(language))
        .flat_map(|(_, lines)| lines)
        .filter_map(|(n, line)| {
            not_lnk(line).map(|why| {
                fail(format!(
                    "{}:{n}: a terminal block has {why}: {}",
                    tree.shown(file),
                    line.trim()
                ))
            })
        })
        .collect()
}

fn file_blocks(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !is_page(tree, file) || tree.generated_exempt("file-blocks", file, text) {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (language, lines) in fences(text) {
        if !FILES.contains(&language) {
            continue;
        }
        let first = lines.iter().position(|(_, line)| !line.trim().is_empty());
        let mut long_string = false;
        for (i, (n, line)) in lines.iter().enumerate() {
            let triple = line.matches("\"\"\"").count() + line.matches("'''").count();
            let inside = long_string;
            if triple % 2 == 1 {
                long_string = !long_string;
            }
            if inside || triple > 0 {
                continue;
            }
            if Some(i) == first && path_comment(line) {
                continue;
            }
            if has_comment(line) {
                found.push(fail(format!(
                    "{}:{n}: a file's block has a comment: {}",
                    tree.shown(file),
                    line.trim()
                )));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terminal_line_is_one_lnk_command_or_the_install() {
        for kept in [
            "lnk env check --spec env.toml",
            "lnk box create aws|gcp",
            "lnk box start <box>",
            "lnk agent chat send main 'a; b | c > d # e'",
            "lnk",
            "curl -fsS https://example.com/health",
            INSTALL,
            "",
        ] {
            assert_eq!(not_lnk(kept), None, "{kept}");
        }
        for (refused, why) in [
            ("cat > env.toml <<'EOF'", "a command that isn't lnk or curl"),
            ("echo hi", "a command that isn't lnk or curl"),
            ("# a comment", "a comment"),
            ("lnk box list  # every box", "a comment"),
            ("lnk box create aws \\", "a line continued with a backslash"),
            ("lnk box list | wc -l", "another command after it"),
            ("lnk box list && lnk box ssh", "another command after it"),
            ("lnk box list --json > boxes.json", "a redirect"),
            ("lnk box list 2>/dev/null", "a redirect"),
            ("lnk env check <<EOF", "a heredoc"),
            ("lnk box ssh $(lnk box list)", "another command inside it"),
            (
                "curl -fsSL https://local.link/install.sh | bash",
                "another command after it",
            ),
            (
                "curl -o out.json https://example.com/a.json > log",
                "a redirect",
            ),
        ] {
            assert_eq!(not_lnk(refused), Some(why), "{refused}");
        }
    }

    #[test]
    fn fences_keep_their_language_and_line_numbers() {
        let page = "# Page\n\n```sh\nlnk box list\n```\n\n  ```toml\n  # env.toml\n  name = \"app\"\n  ```\n";
        let found = fences(page);
        assert_eq!(found[0], ("sh", vec![(4, "lnk box list")]));
        assert_eq!(found[1].0, "toml");
        assert_eq!(found[1].1.len(), 2);
    }

    #[test]
    fn a_files_only_comment_is_its_path_on_its_first_line() {
        assert!(path_comment("# ~/Link/Safe/Agents/main/agent.toml"));
        assert!(path_comment("# env.toml"));
        assert!(!path_comment("# clone.toml: cloning reaches GitHub"));
        assert!(!path_comment("# Variables: not secret"));
        assert!(has_comment("name = \"app\" # its name"));
        assert!(!has_comment("url = \"https://example.com/#top\""));
    }
}
