//! No asides: a parenthesis in a page's prose never holds mainly a link,
//! such as "(see [Boxes](boxes.md))" or "(as [Security](security.md)
//! explains)", which stops the sentence to send the reader elsewhere. A
//! link belongs in the sentence ("each [box](boxes.md) runs") or in a
//! list. A parenthesis is an aside when it holds a link and at most
//! `limits.toml`'s number of other words, its links' own words and its
//! code spans not counted. A parenthesis with no link, such as an
//! abbreviation's expansion or a code name, is never one, and nor is a
//! link's target, code, an HTML comment, or a generated block
//! (`<!-- name:start -->` to `<!-- name:end -->`), which its generator
//! owns.

use std::path::Path;

use crate::{Check, Finding, Rule, Scope, Tree};

pub const RULES: &[Rule] = &[Rule {
    id: "asides",
    scope: Scope::Every,
    fix: "a parenthesis holds mainly a link, an aside sending the reader off: work the link \
          into the sentence (\"each [box](boxes.md) runs\") or into a list",
    check: Check::Each(check),
}];

fn check(tree: &Tree, path: &Path, text: &str) -> Vec<Finding> {
    if path.extension().is_none_or(|e| e != "md")
        || tree.is_data(path)
        || crate::generated_reference(text)
    {
        return Vec::new();
    }
    let at_most = tree.limits.of("asides");
    asides(text, at_most)
        .into_iter()
        .map(|(line, aside)| crate::fail(format!("{}:{line}: {aside}", tree.shown(path))))
        .collect()
}

/// Each aside in `text`'s prose: the line it opens on, from 1, and its
/// words on one line. An aside holds a link and at most `at_most` words
/// besides its links' and its code's.
pub fn asides(text: &str, at_most: usize) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (first, paragraph) in paragraphs(text) {
        found.extend(
            in_paragraph(&paragraph, at_most)
                .into_iter()
                .map(|(line, aside)| (first + line, aside)),
        );
    }
    found
}

/// Each paragraph of prose with the line it starts on: lines outside
/// frontmatter, code blocks and generated blocks, apart at blank lines.
pub(crate) fn paragraphs(text: &str) -> Vec<(usize, String)> {
    let body = crate::md::body(text);
    let skipped = text[..text.len() - body.len()].lines().count();
    let mut found: Vec<(usize, String)> = Vec::new();
    let mut current: Option<(usize, String)> = None;
    let mut fence = false;
    let mut generated: Option<String> = None;
    for (n, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        let prose = if let Some(end) = &generated {
            if trimmed == end {
                generated = None;
            }
            false
        } else if trimmed.starts_with("```") {
            fence = !fence;
            false
        } else if fence {
            false
        } else if let Some(name) = trimmed
            .strip_prefix("<!-- ")
            .and_then(|rest| rest.strip_suffix(":start -->"))
        {
            generated = Some(format!("<!-- {name}:end -->"));
            false
        } else {
            !trimmed.is_empty()
        };
        match (prose, &mut current) {
            (true, Some((_, lines))) => {
                lines.push('\n');
                lines.push_str(line);
            }
            (true, None) => current = Some((skipped + n + 1, line.to_string())),
            (false, _) => found.extend(current.take()),
        }
    }
    found.extend(current);
    found
}

/// A parenthesis open while a paragraph is read.
struct Open {
    at: usize,
    line: usize,
    words: usize,
    links: usize,
}

/// The asides in one paragraph, each with its line from the paragraph's
/// first, from 0.
fn in_paragraph(text: &str, at_most: usize) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    let mut line = 0;
    let mut at = 0;
    let mut in_word = false;
    while let Some(c) = text[at..].chars().next() {
        let skip = match c {
            '\\' => Some(at + 1 + text[at + 1..].chars().next().map_or(0, char::len_utf8)),
            '`' => code_span(text, at),
            '<' if text[at..].starts_with("<!--") => Some(
                text[at..]
                    .find("-->")
                    .map_or(text.len(), |end| at + end + 3),
            ),
            '[' | '!' => link(text, at).inspect(|_| {
                if let Some(innermost) = open.last_mut() {
                    innermost.links += 1;
                }
            }),
            _ => None,
        };
        if let Some(end) = skip {
            line += text[at..end].matches('\n').count();
            at = end;
            in_word = false;
            continue;
        }
        match c {
            '\n' => line += 1,
            '(' => open.push(Open {
                at,
                line,
                words: 0,
                links: 0,
            }),
            ')' => {
                if let Some(closed) = open.pop() {
                    if closed.links > 0 && closed.words <= at_most {
                        let said: Vec<&str> = text[closed.at..=at].split_whitespace().collect();
                        found.push((closed.line, said.join(" ")));
                    }
                    if let Some(outer) = open.last_mut() {
                        outer.words += closed.words;
                    }
                }
            }
            _ => {}
        }
        // A word is a run of letters and digits, an apostrophe within it
        // ("Link's") keeping it one.
        let joins = matches!(c, '\'' | '’')
            && in_word
            && text[at + c.len_utf8()..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric);
        if c.is_alphanumeric()
            && !in_word
            && let Some(innermost) = open.last_mut()
        {
            innermost.words += 1;
        }
        in_word = c.is_alphanumeric() || joins;
        at += c.len_utf8();
    }
    found
}

/// The end of the code span opening at `at`, past its closing backticks:
/// none when the run of backticks is never closed.
pub(crate) fn code_span(text: &str, at: usize) -> Option<usize> {
    let ticks = text[at..].len() - text[at..].trim_start_matches('`').len();
    let fence = &text[at..at + ticks];
    let mut from = at + ticks;
    while let Some(found) = text[from..].find(fence) {
        let start = from + found;
        let run = text[start..].len() - text[start..].trim_start_matches('`').len();
        if run == ticks {
            return Some(start + ticks);
        }
        from = start + run;
    }
    None
}

/// The end of the inline link or image opening at `at`, `[text](target)`
/// or `![text](target)`, past its target's closing parenthesis: none when
/// no link opens there.
fn link(text: &str, at: usize) -> Option<usize> {
    let open = if text[at..].starts_with("![") {
        at + 1
    } else if text[at..].starts_with('[') {
        at
    } else {
        return None;
    };
    let mut depth = 0usize;
    let mut close = None;
    let mut i = open;
    while i < text.len() {
        match text.as_bytes()[i] {
            b'\\' => i += 1,
            b'`' => {
                if let Some(end) = code_span(text, i) {
                    i = end;
                    continue;
                }
            }
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let close = close?;
    if !text[close + 1..].starts_with('(') {
        return None;
    }
    let mut depth = 0usize;
    for (offset, c) in text[close + 1..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(close + 1 + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::asides;

    fn caught(text: &str) -> Vec<String> {
        asides(text, 3)
            .into_iter()
            .map(|(_, aside)| aside)
            .collect()
    }

    /// A parenthesis that holds a link and little else sends the reader
    /// off, whatever its pointing words.
    #[test]
    fn a_parenthesis_holding_mainly_a_link_is_an_aside() {
        for aside in [
            "Each box runs Linux (see [Boxes](boxes.md)).",
            "Each box runs Linux ([Boxes](boxes.md)).",
            "Each box runs Linux (as [Security](security.md) explains).",
            "Each box runs Linux (more in [Boxes](boxes.md#its-disk)).",
            "Each box runs Linux ([Boxes](boxes.md), [Disks](disks.md)).",
            "Each box runs Linux (`lnk box`, [Boxes](boxes.md)).",
            "Each box runs Linux (details\nin [Boxes](boxes.md)).",
            "Each box runs Linux (see [below](#below)).",
            "The protocol ([RFC 9110](https://www.rfc-editor.org/rfc/rfc9110)) applies.",
            "| A cell (see [Boxes](boxes.md)) |",
        ] {
            assert_eq!(caught(aside).len(), 1, "{aside}");
        }
    }

    /// A link in the sentence, an abbreviation's expansion, a
    /// clarification with no link or with words of its own, a link's
    /// target, code and generated blocks are no asides.
    #[test]
    fn a_parenthesis_saying_something_of_its_own_is_no_aside() {
        for kept in [
            "Each [box](boxes.md) runs Linux.",
            "The Agent Client Protocol (ACP) carries it.",
            "Each box runs Linux (or macOS, on a Mac).",
            "Each box runs `lnk-box` (the `box` module).",
            "Read [Boxes (Linux)](boxes.md) first.",
            "Read [Wiki](https://en.wikipedia.org/wiki/Box_(computing)) first.",
            "Keys stay in a file (mode 600, or the [Keychain](k.md) on a Mac).",
            "The `(see [Boxes](boxes.md))` text is code.",
            "```\nIt runs (see [Boxes](boxes.md)).\n```",
            "<!-- docs-tree:start -->\n- Boxes (see [Boxes](boxes.md))\n<!-- docs-tree:end -->",
            "<!-- inventory:boxes:start -->\n(see [Boxes](boxes.md))\n<!-- inventory:boxes:end -->",
            "<!-- (see [Boxes](boxes.md)) -->",
            "---\ndescription: (see [Boxes](boxes.md))\n---\n# Boxes",
            "It runs \\(see [Boxes](boxes.md)\\).",
        ] {
            assert!(caught(kept).is_empty(), "{kept}: {:?}", caught(kept));
        }
    }

    /// Each aside is reported on the line it opens on, its words on one
    /// line, after a generated block and a code block.
    #[test]
    fn an_aside_names_its_line() {
        let text = "# Boxes\n\n```\n(x)\n```\n\nOne line,\nthen (see\n[Boxes](boxes.md)).\n";
        assert_eq!(
            asides(text, 3),
            [(8, "(see [Boxes](boxes.md))".to_string())]
        );
        let fronted = "---\na: b\n---\n# T\n\nIt runs (see [Boxes](boxes.md)).\n";
        assert_eq!(asides(fronted, 3)[0].0, 6);
    }
}
