//! Complete runnable examples and incomplete inline references keep their provenance.

use crate::md;

mod terminals;
pub use terminals::{Discovery, Invocation, SourceSection, TerminalExclusion, terminal_commands};

pub struct Example {
    pub argv: Vec<String>,
    pub complete: bool,
    pub reject: bool,
}

/// Remove shell redirections before operator and quote parsing. Quoted
/// operators remain arguments, and documentation placeholders remain values.
fn redirected(text: &str) -> String {
    let chars: Vec<_> = text.chars().collect();
    let mut kept = String::new();
    let mut word = String::new();
    let mut quote = None;
    let mut at = 0;
    while let Some(&ch) = chars.get(at) {
        if ch == '\\' && quote != Some('\'') {
            word.push(ch);
            at += 1;
            if let Some(&next) = chars.get(at) {
                word.push(next);
                at += 1;
            }
            continue;
        }
        if quote == Some(ch) {
            quote = None;
            word.push(ch);
        } else if quote.is_some() {
            word.push(ch);
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            word.push(ch);
        } else if ch == '<'
            && let Some(end) = chars[at + 1..].iter().position(|ch| *ch == '>')
            && end > 0
            && chars[at + 1..at + end + 1]
                .iter()
                .all(|ch| !ch.is_whitespace() && !matches!(ch, '<' | ';' | '|' | '&'))
        {
            word.extend(&chars[at..at + end + 2]);
            at += end + 2;
            continue;
        } else if matches!(ch, '<' | '>') || (ch == '&' && chars.get(at + 1) == Some(&'>')) {
            if !word.chars().all(|ch| ch.is_ascii_digit()) {
                kept.push_str(&word);
            }
            word.clear();
            kept.push(' ');
            let redirect = if ch == '&' { '>' } else { ch };
            at += usize::from(ch == '&') + 1;
            if chars.get(at) == Some(&redirect) {
                at += 1;
            }
            if chars.get(at).is_some_and(|ch| matches!(ch, '&' | '|')) {
                at += 1;
            }
            while chars.get(at).is_some_and(|ch| ch.is_whitespace()) {
                at += 1;
            }
            // The destination is one shell word, including quoted spaces.
            let mut target_quote = None;
            while let Some(&target) = chars.get(at) {
                if target == '\\' && target_quote != Some('\'') {
                    at = (at + 2).min(chars.len());
                    continue;
                }
                if target_quote == Some(target) {
                    target_quote = None;
                } else if target_quote.is_none() && matches!(target, '\'' | '"') {
                    target_quote = Some(target);
                } else if target_quote.is_none()
                    && (target.is_whitespace() || matches!(target, ';' | '|' | '&' | '<' | '>'))
                {
                    break;
                }
                at += 1;
            }
            continue;
        } else if ch.is_whitespace() || matches!(ch, ';' | '|' | '&') {
            kept.push_str(&word);
            word.clear();
            kept.push(ch);
        } else {
            word.push(ch);
        }
        at += 1;
    }
    kept.push_str(&word);
    kept
}

fn commands(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0;
    for (at, ch) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' && quote != Some('\'') {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if quote.is_none()
            && ch == '#'
            && (at == 0 || text[..at].ends_with(char::is_whitespace))
        {
            parts.push(&text[start..at]);
            return parts;
        } else if quote.is_none() && matches!(ch, ';' | '|' | '&') {
            parts.push(&text[start..at]);
            start = at + ch.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Shell quoting at a logical line boundary, with unquoted comments ignored.
fn shell_state(text: &str) -> (Option<char>, bool) {
    let mut quote = None;
    let mut escaped = false;
    let mut previous = None;
    for ch in text.chars() {
        if escaped {
            escaped = false;
        } else if ch == '\\' && quote != Some('\'') {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if quote.is_none() && ch == '#' && previous.is_none_or(char::is_whitespace) {
            break;
        }
        previous = Some(ch);
    }
    (quote, escaped)
}

fn mentions_link(text: &str) -> bool {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.')))
        .any(|word| word == "lnk")
}

/// Shell substitutions need a shell parser before their nested invocations can
/// have complete source identities. Refuse Link-bearing ones explicitly.
fn link_substitution(text: &str) -> bool {
    let chars: Vec<_> = text.chars().collect();
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut backtick = false;
    let mut word = String::new();
    let mut at = 0;
    while let Some(&ch) = chars.get(at) {
        if escaped {
            escaped = false;
            at += 1;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            at += 1;
            continue;
        }
        if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if quote != Some('\'') {
            if quote.is_none() && ch == '#' && (at == 0 || chars[at - 1].is_whitespace()) {
                break;
            }
            if ch == '$' && chars.get(at + 1) == Some(&'(') {
                depth += 1;
                word.clear();
                at += 2;
                continue;
            }
            if ch == '`' {
                if backtick && word == "lnk" {
                    return true;
                }
                backtick = !backtick;
                word.clear();
            }
            if depth > 0 || backtick {
                if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.') {
                    word.push(ch);
                } else {
                    if word == "lnk" {
                        return true;
                    }
                    word.clear();
                }
            }
            if ch == ')' && depth > 0 {
                depth -= 1;
            }
        }
        at += 1;
    }
    (depth > 0 || backtick) && word == "lnk"
}

struct ParsedLine {
    examples: Vec<Example>,
    exclusions: Vec<(terminals::ExclusionReason, Vec<String>)>,
}

fn introspection(words: &[String]) -> bool {
    let Some((program, rest)) = words.split_first() else {
        return false;
    };
    let options: &[&str] = match program.as_str() {
        "type" => &["-a", "-p", "-P", "-t"],
        "which" => &["-a"],
        "command" => &["-v", "-V", "-p"],
        _ => return false,
    };
    let mut rest = rest;
    let mut lookup = program != "command";
    while let Some((word, more)) = rest.split_first() {
        if word == "--" {
            rest = more;
            break;
        }
        if !word.starts_with('-') {
            break;
        }
        if !options.contains(&word.as_str()) {
            return false;
        }
        lookup |= matches!(word.as_str(), "-v" | "-V");
        rest = more;
    }
    lookup
        && !rest.is_empty()
        && rest.iter().any(|word| word == "lnk")
        && rest.iter().all(|word| !word.starts_with('-'))
}

fn cargo_artifact_selection(words: &[String]) -> bool {
    if words.first().is_none_or(|word| word != "cargo")
        || words
            .get(1)
            .is_none_or(|word| !matches!(word.as_str(), "build" | "check" | "test" | "clippy"))
    {
        return false;
    }
    let switches = [
        "--release",
        "-r",
        "--workspace",
        "--all",
        "--all-targets",
        "--all-features",
        "--no-default-features",
        "--locked",
        "--frozen",
        "--offline",
        "--quiet",
        "-q",
        "--verbose",
        "-v",
        "--lib",
        "--bins",
        "--tests",
        "--examples",
        "--benches",
        "--no-run",
        "--no-fail-fast",
    ];
    let values = [
        "--manifest-path",
        "--target-dir",
        "--target",
        "--package",
        "-p",
        "--exclude",
        "--features",
        "-F",
        "--jobs",
        "-j",
        "--profile",
        "--message-format",
        "--color",
        "--bin",
        "--example",
        "--test",
        "--bench",
    ];
    let mut selected = false;
    let mut at = 2;
    while let Some(word) = words.get(at) {
        if word == "--" {
            return selected && at + 1 == words.len();
        }
        if switches.contains(&word.as_str()) {
            at += 1;
            continue;
        }
        if let Some((flag, value)) = word.split_once('=') {
            if !values.contains(&flag) || value.is_empty() {
                return false;
            }
            selected |= flag == "--bin" && value == "lnk";
            at += 1;
            continue;
        }
        if !values.contains(&word.as_str()) {
            return false;
        }
        let Some(value) = words.get(at + 1).filter(|value| !value.starts_with('-')) else {
            return false;
        };
        selected |= word == "--bin" && value == "lnk";
        at += 2;
    }
    selected
}

fn parsed_line(text: &str, complete: bool, reject: bool) -> Result<ParsedLine, String> {
    if complete && link_substitution(text) {
        return Err("Link-bearing shell substitution needs a reviewed interpretation".into());
    }
    if shlex::split(text).is_none() {
        return if text.split_whitespace().any(|word| word == "lnk") {
            Err("unfinished shell quoting in a Link example".into())
        } else {
            Ok(ParsedLine {
                examples: vec![],
                exclusions: vec![],
            })
        };
    }
    let mut examples = Vec::new();
    let mut exclusions = Vec::new();
    if complete && let Some(words) = shlex::split(text) {
        let command_string = words.iter().enumerate().any(|(at, word)| {
            if word == "eval" {
                return words[at + 1..].iter().any(|part| mentions_link(part));
            }
            let flag = word == "--command"
                || word.starts_with('-') && !word.starts_with("--") && word.contains('c');
            flag && (words.get(at + 1).is_some_and(|part| mentions_link(part))
                || word.strip_prefix("-c").is_some_and(mentions_link))
        });
        if command_string {
            return Err("Link-bearing shell command string needs a reviewed interpretation".into());
        }
    }
    let text = redirected(text);
    for command in commands(&text) {
        let words = shlex::split(command).ok_or("unfinished shell quoting in a Link example")?;
        if cargo_artifact_selection(&words) {
            exclusions.push((terminals::ExclusionReason::CargoArtifactSelection, words));
            continue;
        }
        if introspection(&words) {
            exclusions.push((terminals::ExclusionReason::ShellIntrospection, words));
            continue;
        }
        if words.first().is_some_and(|word| word == "command")
            && words
                .iter()
                .any(|word| matches!(word.as_str(), "-v" | "-V"))
            && words.iter().any(|word| word == "lnk")
        {
            return Err(
                "unsupported Link-bearing shell introspection needs a reviewed interpretation"
                    .into(),
            );
        }
        let words = if words.first().is_some_and(|word| word == "strace") {
            let inner = words
                .iter()
                .position(|word| word == "--")
                .map(|separator| &words[separator + 1..]);
            match inner {
                Some(inner) if inner.first().is_some_and(|word| word == "lnk") => inner,
                _ if words.iter().any(|word| word == "lnk") => {
                    return Err(
                        "strace needs an explicit -- before a direct Link invocation".into(),
                    );
                }
                _ => continue,
            }
        } else {
            words.as_slice()
        };
        if let Some(begin) = words.iter().position(|word| word == "lnk") {
            let prefix = &words[..begin];
            if prefix.iter().any(|word| word == "strace") {
                return Err(
                    "unsupported prefix before strace needs a reviewed interpretation".into(),
                );
            }
            let assignments = prefix.iter().all(|word| {
                word.split_once('=').is_some_and(|(name, _)| {
                    !name.is_empty()
                        && name
                            .chars()
                            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
                })
            });
            let wrapper = prefix.first().is_some_and(|word| {
                matches!(word.as_str(), "env" | "sudo" | "command" | "exec" | "time")
            });
            if complete && !assignments && !wrapper {
                if prefix
                    .first()
                    .is_some_and(|word| matches!(word.as_str(), "echo" | "printf"))
                {
                    continue;
                }
                return Err("unsupported shell prefix before a Link invocation needs a reviewed interpretation".into());
            }
            examples.push(Example {
                argv: words[begin..].to_vec(),
                complete,
                reject,
            });
        }
    }
    Ok(ParsedLine {
        examples,
        exclusions,
    })
}

fn line(text: &str, complete: bool, reject: bool) -> Result<Vec<Example>, String> {
    Ok(parsed_line(text, complete, reject)?.examples)
}

pub fn read(text: &str) -> Result<Vec<Example>, String> {
    let (prose, _) = md::split(text);
    let mut examples = Vec::new();
    for reference in md::spans(&prose.join("\n")) {
        examples.extend(line(&reference, false, false)?);
    }
    let found = terminal_commands(text);
    if !found.problems.is_empty() {
        return Err(found.problems.join("; "));
    }
    examples.extend(found.invocations.into_iter().map(|invocation| Example {
        argv: invocation.argv,
        complete: true,
        reject: invocation.reject,
    }));
    Ok(examples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirections_are_shell_syntax_and_quoted_operators_are_arguments() {
        for suffix in [
            ">file",
            "> file",
            "2>&1",
            "< file",
            "2> file",
            ">> 'two words'",
            "&>file",
        ] {
            let source = format!("```sh\nlnk test run <target> {suffix}\n```\n");
            assert_eq!(
                read(&source).unwrap()[0].argv,
                ["lnk", "test", "run", "<target>"],
                "{suffix}"
            );
        }
        let examples = read("```sh\nlnk test run '>file' \"< file\" --text='2>&1' '|' ';' '&' >out | lnk test run next <in\n```\n").unwrap();
        assert_eq!(
            examples[0].argv,
            [
                "lnk",
                "test",
                "run",
                ">file",
                "< file",
                "--text=2>&1",
                "|",
                ";",
                "&"
            ]
        );
        assert_eq!(examples[1].argv, ["lnk", "test", "run", "next"]);
        assert_eq!(
            read("`lnk test run <target> --port <port>`").unwrap()[0].argv,
            ["lnk", "test", "run", "<target>", "--port", "<port>"]
        );
        assert!(read("```sh\nlnk test run x > \"unfinished\n```\n").is_err());
        assert_eq!(
            read("```sh\nlnk test run x # comment; lnk ignored\n```\n")
                .unwrap()
                .len(),
            1
        );
    }
}
