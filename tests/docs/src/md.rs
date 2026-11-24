//! Reading Markdown as the checks do: prose and code blocks apart,
//! headings, links, `lnk` commands and a hub's words.

use std::collections::BTreeSet;

/// Frontmatter describes a page but is not part of its visible prose.
pub fn body(text: &str) -> &str {
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return text;
    };
    if first.trim() != "---" {
        return text;
    }
    let mut offset = first.len();
    for line in lines {
        offset += line.len();
        if matches!(line.trim(), "---" | "...") {
            return &text[offset..];
        }
    }
    text
}

/// Each line outside code blocks, and the code blocks' lines with their
/// block's info string (`sh`, `""`, ...).
pub fn split(text: &str) -> (Vec<&str>, Vec<(&str, &str)>) {
    let (mut prose, mut code) = (Vec::new(), Vec::new());
    let mut fence: Option<&str> = None;
    for line in body(text).lines() {
        match (fence, line.trim_start().strip_prefix("```")) {
            (None, Some(info)) => fence = Some(info.trim()),
            (Some(_), Some(_)) => fence = None,
            (Some(info), None) => code.push((info, line)),
            (None, None) => prose.push(line),
        }
    }
    (prose, code)
}

/// GitHub's anchor for a heading: lower case, punctuation dropped, spaces
/// to hyphens.
pub fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

pub fn anchors(text: &str) -> BTreeSet<String> {
    split(text)
        .0
        .iter()
        .filter(|l| l.starts_with('#'))
        .map(|l| slug(l.trim_start_matches('#')))
        .collect()
}

/// Each heading outside code blocks: its level and its text.
pub fn headings<'a>(prose: &[&'a str]) -> Vec<(usize, &'a str)> {
    prose
        .iter()
        .filter_map(|line| {
            let level = line.chars().take_while(|c| *c == '#').count();
            (level > 0 && level <= 6 && line[level..].starts_with(' '))
                .then(|| (level, line[level..].trim()))
        })
        .collect()
}

/// The targets of `[text](target)` links in these lines.
pub fn links<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let mut found = Vec::new();
    for line in lines {
        let mut rest = *line;
        while let Some(at) = rest.find("](") {
            rest = &rest[at + 2..];
            let end = rest.find([')', ' ']).unwrap_or(rest.len());
            found.push(&rest[..end]);
            rest = &rest[end..];
        }
    }
    found
}

/// A file's words outside its links' targets, its headings and its
/// markup, and outside the text of its links.
pub fn words(text: &str) -> usize {
    let mut rest = text.to_string();
    // [text](target) and **[text](target)** say nothing of their own.
    while let Some(start) = rest.find('[') {
        let Some(mid) = rest[start..].find("](") else {
            break;
        };
        let Some(end) = rest[start + mid..].find(')') else {
            break;
        };
        rest.replace_range(start..start + mid + end + 1, " ");
    }
    rest.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("|---"))
        .flat_map(|l| l.split_whitespace())
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count()
}

/// `(group, command, next)` for each `lnk <group> <command> <next>` in a
/// shell line, the command and the word after it empty when none follows:
/// `lnk agent harness <verb>` names its verb third.
pub fn commands(line: &str) -> Vec<(String, String, String)> {
    let line = line.split(" #").next().unwrap_or(line);
    let word = |w: &&str| {
        w.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && w.chars().all(|c| c.is_ascii_lowercase() || c == '-')
    };
    let words: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | '$'))
        .collect();
    let mut found = Vec::new();
    for (i, w) in words.iter().enumerate() {
        if *w != "lnk" {
            continue;
        }
        let rest: Vec<&str> = words[i + 1..]
            .iter()
            .filter(|w| !w.is_empty())
            .take(3)
            .copied()
            .collect();
        let as_word = |w: Option<&&str>| match w {
            Some(w) if word(w) => w.to_string(),
            _ => String::new(),
        };
        match rest.as_slice() {
            [group, ..] if word(group) => {
                let command = as_word(rest.get(1));
                let next = match command.is_empty() {
                    true => String::new(),
                    false => as_word(rest.get(2)),
                };
                found.push((group.to_string(), command, next))
            }
            _ => {}
        }
    }
    found
}

/// The code spans in Markdown prose that run `lnk` (`` `lnk agent
/// start` ``), each on one line: a span may wrap onto the next line, but
/// never leaves its paragraph. Bare words are left alone: prose says "lnk
/// drops it" as often as it gives a command.
pub fn spans(prose: &str) -> Vec<String> {
    let mut found = Vec::new();
    for paragraph in prose.split("\n\n") {
        let mut rest = paragraph;
        while let Some(open) = rest.find('`') {
            let ticks = rest[open..].len() - rest[open..].trim_start_matches('`').len();
            let after = &rest[open + ticks..];
            let Some(close) = after.find(&rest[open..open + ticks]) else {
                break;
            };
            let span = after[..close].split_whitespace().collect::<Vec<_>>();
            if span.first() == Some(&"lnk") {
                found.push(span.join(" "));
            }
            rest = after[close + ticks..].trim_start_matches('`');
        }
    }
    found
}

/// Every line of a page that may run `lnk`: its prose's code spans, and
/// its shell blocks' lines with the spans in their comments.
pub fn shell_lines(text: &str) -> Vec<String> {
    let (prose, code) = split(text);
    let mut lines = spans(&prose.join("\n"));
    for (info, line) in code {
        if !matches!(info, "" | "sh" | "bash" | "console") {
            continue;
        }
        // A comment, on its own line or after a command, is prose.
        match line.trim_start().starts_with('#') {
            true => lines.extend(spans(line)),
            false => {
                lines.push(line.to_string());
                lines.extend(spans(line.split_once(" #").map_or("", |(_, c)| c)));
            }
        }
    }
    lines
}

/// Each ```toml block of `text`.
pub fn toml_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut block: Option<String> = None;
    for line in text.lines() {
        match (&mut block, line.trim_start()) {
            (None, "```toml") => block = Some(String::new()),
            (Some(_), "```") => blocks.extend(block.take()),
            (Some(lines), _) => {
                lines.push_str(line);
                lines.push('\n');
            }
            (None, _) => {}
        }
    }
    blocks
}

/// Decode a Markdown URL's path before resolving it on disk. Invalid
/// escapes stay literal, and decoding happens once (so %2520 names %20).
pub fn link_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            let high = (bytes[at + 1] as char).to_digit(16);
            let low = (bytes[at + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                decoded.push((high * 16 + low) as u8);
                at += 3;
                continue;
            }
        }
        decoded.push(bytes[at]);
        at += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| path.to_owned())
}
