//! The rules of a page's shape: a paragraph under every heading, one
//! paragraph a section, and paragraphs, table cells and list items no
//! longer than their limits. A page written before them is listed in its
//! workspace's `large-files.toml` under `unshaped`, which may shrink,
//! never grow.

use std::path::Path;

use crate::site::is_page;
use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[
    Rule {
        id: "heading-paragraph",
        scope: Scope::Site,
        fix: "a heading opens with a paragraph, before any list, table, code block or the \
              next heading: say what the section is for",
        check: Check::Each(heading_paragraph),
    },
    Rule {
        id: "one-paragraph",
        scope: Scope::Site,
        fix: "two paragraphs in a row are two subjects: put a heading between them",
        check: Check::Each(one_paragraph),
    },
    Rule {
        id: "paragraph-length",
        scope: Scope::Site,
        fix: "a paragraph past its limit holds more than one thing: split it into \
              subsections, each a heading and a paragraph",
        check: Check::Each(paragraph_length),
    },
    Rule {
        id: "cell-length",
        scope: Scope::Site,
        fix: "a table cell is a few words: a row that needs explaining is a subsection, its \
              short cell the heading and its long cell the paragraph",
        check: Check::Each(cell_length),
    },
    Rule {
        id: "item-length",
        scope: Scope::Site,
        fix: "a list item is a line or two: one that needs a paragraph is a subsection",
        check: Check::Each(item_length),
    },
    Rule {
        id: "list-items",
        scope: Scope::Site,
        fix: "a list is three or more like things, or links: fewer fit in a sentence",
        check: Check::Each(list_items),
    },
    Rule {
        id: "table-rows",
        scope: Scope::Site,
        fix: "a table compares two rows or more: one row is a paragraph",
        check: Check::Each(table_rows),
    },
    Rule {
        id: "table-code",
        scope: Scope::Site,
        fix: "keys, flags and commands are each a subsection, their name the heading and a \
              paragraph under it, so a page can link to one: not a table's rows",
        check: Check::Each(table_code),
    },
    Rule {
        id: "table-articles",
        scope: Scope::Site,
        fix: "a row is scanned by its first word: start each first cell with the word that \
              names it, not \"a\", \"an\" or \"the\" (one catch-all row may)",
        check: Check::Each(table_articles),
    },
    Rule {
        id: "bare-command",
        scope: Scope::Site,
        fix: "a command is code: put each `lnk` command in backticks or a code block",
        check: Check::Each(bare_command),
    },
    Rule {
        id: "bare-variable",
        scope: Scope::Site,
        fix: "an environment variable is code: put it in backticks",
        check: Check::Each(bare_variable),
    },
    Rule {
        id: "bare-path",
        scope: Scope::Site,
        fix: "a path is code: put it in backticks",
        check: Check::Each(bare_path),
    },
    Rule {
        id: "thin-section",
        scope: Scope::Site,
        fix: "a section that holds only a short paragraph is too thin to stand alone: fold it \
              into the section beside it, or, if it only points elsewhere, make its page a \
              list of links",
        check: Check::Each(thin_section),
    },
    Rule {
        id: "unshaped",
        scope: Scope::Site,
        fix: "large-files.toml's unshaped lists only pages that break a shape rule: take out \
              these",
        check: Check::Whole(unshaped),
    },
];

/// A page's parts, as a reader sees them.
#[derive(Debug, PartialEq)]
pub enum Block {
    /// Its level, the count of its `#`, and its text.
    Heading(usize, String),
    Paragraph(String),
    /// Each item's text.
    List(Vec<String>),
    /// Each row's cells, the separator row left out.
    Table(Vec<Vec<String>>),
    /// A quote: right under a page's title, its hook.
    Quote(String),
    /// A code block or an HTML comment.
    Other,
}

/// Whether `paragraph` is only a link the website makes a component of:
/// one titled "button", "sponsors" or "offers".
fn component(paragraph: &str) -> bool {
    paragraph.starts_with('[')
        && ["button", "sponsors", "offers"]
            .iter()
            .any(|kind| paragraph.ends_with(&format!(" \"{kind}\")")))
        && paragraph.matches("](").count() == 1
}

/// `text`'s blocks, in order.
pub fn blocks(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut lines = crate::md::body(text).lines().peekable();
    let mut generated = false;
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == crate::navigation::START {
            generated = true;
        } else if trimmed == crate::navigation::END {
            generated = false;
        } else if generated && crate::navigation::is_docs_heading(trimmed) {
            // The navigation rule validates this complete generated block.
            // Its link list needs no boilerplate introductory paragraph.
            continue;
        }
        if trimmed.starts_with("```") {
            for next in lines.by_ref() {
                if next.trim_start().starts_with("```") {
                    break;
                }
            }
            out.push(Block::Other);
        } else if trimmed.starts_with("<!--") {
            let mut closed = trimmed.contains("-->");
            while !closed {
                match lines.next() {
                    Some(next) => closed = next.contains("-->"),
                    None => break,
                }
            }
            out.push(Block::Other);
        } else if cards_delimiter(trimmed)
            || (trimmed.len() >= 3 && trimmed.trim_end().chars().all(|c| c == '-'))
        {
            // `---`: a break between a page's cards.
            out.push(Block::Other);
        } else if let Some(heading) = heading(line) {
            let level = line.len() - line.trim_start_matches('#').len();
            out.push(Block::Heading(level, heading.to_string()));
        } else if let Some(first) = trimmed.strip_prefix('>') {
            let mut quote = first.trim().to_string();
            while let Some(next) = lines.peek().and_then(|l| l.trim_start().strip_prefix('>')) {
                quote.push(' ');
                quote.push_str(next.trim());
                lines.next();
            }
            out.push(Block::Quote(quote.trim().to_string()));
        } else if trimmed.starts_with('|') {
            let mut rows = vec![cells(trimmed)];
            while let Some(next) = lines.peek().map(|l| l.trim_start()) {
                if !next.starts_with('|') {
                    break;
                }
                rows.push(cells(next));
                lines.next();
            }
            rows.retain(|row| {
                !row.iter()
                    .all(|c| !c.is_empty() && c.chars().all(|c| "-: ".contains(c)))
            });
            out.push(Block::Table(rows));
        } else if let Some(first) = item(line) {
            let mut items = vec![first.to_string()];
            while let Some(next) = lines.peek().copied() {
                if next.trim().is_empty()
                    || heading(next).is_some()
                    || next.trim_start().starts_with("```")
                    || cards_delimiter(next.trim())
                {
                    break;
                }
                match item(next) {
                    Some(text) => items.push(text.to_string()),
                    None => {
                        let last = items.last_mut().expect("an item");
                        last.push(' ');
                        last.push_str(next.trim());
                    }
                }
                lines.next();
            }
            out.push(Block::List(items));
        } else {
            let mut paragraph = trimmed.to_string();
            while let Some(next) = lines.peek().copied() {
                let t = next.trim_start();
                if t.is_empty()
                    || heading(next).is_some()
                    || t.starts_with("```")
                    || t.starts_with('|')
                    || item(next).is_some()
                    || cards_delimiter(t.trim_end())
                {
                    break;
                }
                paragraph.push(' ');
                paragraph.push_str(next.trim());
                lines.next();
            }
            // A link alone, titled what the site makes of it: a component.
            if component(&paragraph) {
                out.push(Block::Other);
                continue;
            }
            // Indented under a list, after a blank line: its item's.
            match out.last_mut() {
                Some(Block::List(items)) if line.starts_with("  ") => {
                    let last = items.last_mut().expect("an item");
                    last.push(' ');
                    last.push_str(&paragraph);
                }
                _ => out.push(Block::Paragraph(paragraph)),
            }
        }
    }
    // A list with blank lines between its items is one list.
    let mut merged: Vec<Block> = Vec::new();
    for block in out {
        match (merged.last_mut(), block) {
            (Some(Block::List(items)), Block::List(more)) => items.extend(more),
            (_, block) => merged.push(block),
        }
    }
    merged
}

fn cards_delimiter(line: &str) -> bool {
    matches!(line.trim_end(), ":::cards" | ":::")
}

fn heading(line: &str) -> Option<&str> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    (hashes > 0 && line[hashes..].starts_with(' ')).then(|| line[hashes..].trim())
}

fn item(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
        return Some(rest);
    }
    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    (digits > 0)
        .then(|| t[digits..].strip_prefix(". "))
        .flatten()
}

fn cells(row: &str) -> Vec<String> {
    let row = row.trim().trim_start_matches('|').trim_end_matches('|');
    row.split('|').map(|c| c.trim().to_string()).collect()
}

/// What a reader reads of `text`: link targets left out, spaces one.
pub fn characters(text: &str) -> usize {
    let mut rest = text.to_string();
    while let Some(start) = rest.find("](") {
        let Some(end) = rest[start..].find(')') else {
            break;
        };
        rest.replace_range(start + 1..start + end + 1, "");
    }
    rest.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .count()
}

/// The first words of `text`, to name it in a finding.
pub fn named(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().take(6).collect();
    format!("\"{}...\"", words.join(" "))
}

/// Whether `file` is a page the shape rules hold: a page of the website,
/// not listed as written before them.
pub fn shaped(tree: &Tree, file: &Path) -> bool {
    is_page(tree, file) && !tree.large.unshaped.contains(&tree.shown(file))
}

fn heading_paragraph(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !shaped(tree, file) || tree.generated_exempt("heading-paragraph", file, text) {
        return Vec::new();
    }
    find_heading_paragraph(&blocks(text))
        .into_iter()
        .map(|h| {
            fail(format!(
                "{}: \"{h}\" has no paragraph under it",
                tree.shown(file)
            ))
        })
        .collect()
}

fn find_heading_paragraph(blocks: &[Block]) -> Vec<String> {
    // A hook under the title stands between it and its paragraph.
    let title = blocks.iter().position(|b| matches!(b, Block::Heading(..)));
    let hooked = crate::pages::hook(blocks).is_some();
    blocks
        .iter()
        .enumerate()
        .filter_map(|(i, block)| {
            let skip = usize::from(Some(i) == title && hooked);
            match (block, blocks.get(i + 1 + skip)) {
                (Block::Heading(_, h), next) if !matches!(next, Some(Block::Paragraph(_))) => {
                    Some(h.clone())
                }
                _ => None,
            }
        })
        .collect()
}

fn one_paragraph(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    // A generated spec page's field entry is its declaration's
    // description, whose paragraphs are one field's.
    if !shaped(tree, file) || tree.generated_exempt("one-paragraph", file, text) {
        return Vec::new();
    }
    find_one_paragraph(&blocks(text))
        .into_iter()
        .map(|p| {
            fail(format!(
                "{}: {} follows a paragraph",
                tree.shown(file),
                named(&p)
            ))
        })
        .collect()
}

fn find_one_paragraph(blocks: &[Block]) -> Vec<String> {
    blocks
        .windows(2)
        .filter_map(|pair| match pair {
            [Block::Paragraph(_), Block::Paragraph(p)] => Some(p.clone()),
            _ => None,
        })
        .collect()
}

/// Each piece of `blocks` the rule `id` measures.
fn measured(blocks: &[Block], id: &str) -> Vec<String> {
    let mut out = Vec::new();
    for block in blocks {
        match (block, id) {
            (Block::Paragraph(p), "paragraph-length") => out.push(p.clone()),
            (Block::Table(rows), "cell-length") => out.extend(rows.iter().flatten().cloned()),
            (Block::List(items), "item-length") => out.extend(items.iter().cloned()),
            _ => {}
        }
    }
    out
}

fn find_long(tree: &Tree, blocks: &[Block], id: &str) -> Vec<(String, usize)> {
    let most = tree.limits.of(id);
    measured(blocks, id)
        .into_iter()
        .map(|t| {
            let n = characters(&t);
            (t, n)
        })
        .filter(|(_, n)| *n > most)
        .collect()
}

fn long(tree: &Tree, file: &Path, text: &str, id: &str) -> Vec<Finding> {
    if !shaped(tree, file) {
        return Vec::new();
    }
    let most = tree.limits.of(id);
    find_long(tree, &blocks(text), id)
        .into_iter()
        .map(|(t, n)| {
            fail(format!(
                "{}: {}: {n} characters, over {most}",
                tree.shown(file),
                named(&t)
            ))
        })
        .collect()
}

fn paragraph_length(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    long(tree, file, text, "paragraph-length")
}

fn cell_length(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    long(tree, file, text, "cell-length")
}

fn item_length(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    // A generated page's entry is one declaration's help, as long as it
    // is (`limits.toml`).
    if tree.generated_exempt("item-length", file, text) {
        return Vec::new();
    }
    long(tree, file, text, "item-length")
}

/// Whether `item` is only a link, and what's said of it.
fn is_link(item: &str) -> bool {
    let t = item.trim_start_matches("**");
    t.starts_with('[') && t.contains("](")
}

fn find_list_items(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::List(items) if items.len() < 3 && !items.iter().all(|i| is_link(i)) => {
                Some(items[0].clone())
            }
            _ => None,
        })
        .collect()
}

fn list_items(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !shaped(tree, file) {
        return Vec::new();
    }
    find_list_items(&blocks(text))
        .into_iter()
        .map(|i| {
            fail(format!(
                "{}: a list of fewer than three, {}",
                tree.shown(file),
                named(&i)
            ))
        })
        .collect()
}

/// Each table of fewer than two rows under its header, by its first cell.
fn find_table_rows(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Table(rows) if rows.len() < 3 => Some(
                rows.first()
                    .and_then(|r| r.first())
                    .cloned()
                    .unwrap_or_default(),
            ),
            _ => None,
        })
        .collect()
}

fn table_rows(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !shaped(tree, file) {
        return Vec::new();
    }
    find_table_rows(&blocks(text))
        .into_iter()
        .map(|c| {
            fail(format!(
                "{}: a table of one row, {}",
                tree.shown(file),
                named(&c)
            ))
        })
        .collect()
}

/// Each table whose rows under its header each start with inline code.
fn find_table_code(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Table(rows)
                if rows.len() > 1
                    && rows[1..]
                        .iter()
                        .all(|r| r.first().is_some_and(|c| c.starts_with('`'))) =>
            {
                Some(rows[1][0].clone())
            }
            _ => None,
        })
        .collect()
}

fn table_code(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !shaped(tree, file) {
        return Vec::new();
    }
    find_table_code(&blocks(text))
        .into_iter()
        .map(|c| fail(format!("{}: a table of code, from {c}", tree.shown(file))))
        .collect()
}

/// Whether `cell` opens with an article, which a reader scans past.
fn articled(cell: &str) -> bool {
    let first = cell
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .split_whitespace()
        .next()
        .unwrap_or("");
    ["a", "an", "the"].contains(&first.to_lowercase().as_str())
}

/// Each table where more than one row's first cell opens with an article.
fn find_table_articles(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Table(rows) => {
                let articled: Vec<&String> = rows
                    .iter()
                    .skip(1)
                    .filter_map(|r| r.first())
                    .filter(|c| articled(c))
                    .collect();
                (articled.len() > 1).then(|| articled[0].clone())
            }
            _ => None,
        })
        .collect()
}

fn table_articles(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !shaped(tree, file) {
        return Vec::new();
    }
    find_table_articles(&blocks(text))
        .into_iter()
        .map(|c| {
            fail(format!(
                "{}: rows open with articles, from {}",
                tree.shown(file),
                named(&c)
            ))
        })
        .collect()
}

/// Whether the page `text` breaks any shape rule.
fn breaks(tree: &Tree, text: &str) -> bool {
    let blocks = blocks(text);
    !find_heading_paragraph(&blocks).is_empty()
        || !find_one_paragraph(&blocks).is_empty()
        || !find_list_items(&blocks).is_empty()
        || !find_table_rows(&blocks).is_empty()
        || !find_table_code(&blocks).is_empty()
        || !find_table_articles(&blocks).is_empty()
        || !find_thin(tree, &blocks).is_empty()
        || crate::pages::has_troubleshooting(&blocks)
        || !find_bare(text, "bare-command").is_empty()
        || !find_bare(text, "bare-variable").is_empty()
        || !find_bare(text, "bare-path").is_empty()
        || ["paragraph-length", "cell-length", "item-length"]
            .iter()
            .any(|id| !find_long(tree, &blocks, id).is_empty())
}

fn unshaped(tree: &Tree) -> Vec<Finding> {
    tree.large
        .unshaped
        .iter()
        .filter(|shown| {
            tree.files
                .get(&tree.root.join(shown))
                .is_none_or(|text| !breaks(tree, text))
        })
        .map(|shown| fail(format!("{shown}: breaks no shape rule, or is gone")))
        .collect()
}

/// `text`'s prose as a reader sees it outside code: code blocks, inline
/// code, link targets and HTML comments left out.
fn plain(text: &str) -> String {
    let prose = crate::md::split(text).0.join("\n");
    let mut out = String::new();
    let mut rest = prose.as_str();
    while let Some(start) = rest.find(['`', '<', ']']) {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let skip = if after.starts_with('`') {
            // A run of backticks closes at the same run.
            let run = after.len() - after.trim_start_matches('`').len();
            let fence = &after[..run];
            after[run..].find(fence).map(|end| run + end + run)
        } else if after.starts_with("<!--") {
            after.find("-->").map(|end| end + 3)
        } else if after.starts_with('<') {
            after.find('>').map(|end| end + 1)
        } else if after.starts_with("](") {
            after.find(')').map(|end| end + 1)
        } else {
            None
        };
        match skip {
            Some(n) => {
                out.push(' ');
                rest = &after[n..];
            }
            None => {
                out.push_str(&after[..1]);
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Each bare piece of code the rule `id` finds in `text`'s prose.
fn find_bare(text: &str, id: &str) -> Vec<String> {
    let plain = plain(text);
    let words: Vec<&str> = plain.split_whitespace().collect();
    let bare = |w: &str| {
        w.trim_matches(|c: char| ",.;:!?()[]*\"'".contains(c))
            .to_string()
    };
    let mut found = Vec::new();
    for (i, word) in words.iter().enumerate() {
        let w = bare(word);
        let hit = match id {
            "bare-command" => {
                w == "lnk"
                    && words
                        .get(i + 1)
                        .is_some_and(|next| next.starts_with(|c: char| c.is_ascii_lowercase()))
            }
            "bare-variable" => {
                w.contains('_')
                    && w.starts_with(|c: char| c.is_ascii_uppercase())
                    && w.chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            }
            "bare-path" => {
                ["~/", "./", "/"].iter().any(|p| w.starts_with(p))
                    && w[1..].contains('/')
                    && !w.starts_with("//")
            }
            _ => false,
        };
        if hit {
            found.push(match id {
                "bare-command" => format!("lnk {}", words.get(i + 1).copied().unwrap_or("")),
                _ => w,
            });
        }
    }
    found
}

fn bare(tree: &Tree, file: &Path, text: &str, id: &str) -> Vec<Finding> {
    if !shaped(tree, file) {
        return Vec::new();
    }
    find_bare(text, id)
        .into_iter()
        .map(|w| fail(format!("{}: {w}", tree.shown(file))))
        .collect()
}

fn bare_command(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    bare(tree, file, text, "bare-command")
}

fn bare_variable(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    bare(tree, file, text, "bare-variable")
}

fn bare_path(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    bare(tree, file, text, "bare-path")
}

/// Sections every page may hold however short they are: a fixed line.
const STANDARD: &[&str] = &["License"];

/// Each section, not the page's title, that holds nothing but one
/// paragraph shorter than the limit, by its heading: no subsection under
/// it, so the next heading is at its level or above, or the page ends.
fn find_thin(tree: &Tree, blocks: &[Block]) -> Vec<String> {
    let most = tree.limits.of("thin-section");
    let title = blocks.iter().position(|b| matches!(b, Block::Heading(..)));
    let mut found = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        let (Block::Heading(level, h), Some(Block::Paragraph(p))) = (block, blocks.get(i + 1))
        else {
            continue;
        };
        let ends = match blocks.get(i + 2) {
            None => true,
            Some(Block::Heading(next, _)) => next <= level,
            Some(_) => false,
        };
        if ends && Some(i) != title && !STANDARD.contains(&h.as_str()) && characters(p) < most {
            found.push(h.clone());
        }
    }
    found
}

fn thin_section(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    // A generated page's sections are as long as its declarations make
    // them: an index of two parts says no more than their names
    // (`limits.toml`).
    if !shaped(tree, file) || tree.generated_exempt("thin-section", file, text) {
        return Vec::new();
    }
    let most = tree.limits.of("thin-section");
    find_thin(tree, &blocks(text))
        .into_iter()
        .map(|h| {
            fail(format!(
                "{}: \"{h}\" holds one paragraph under {most} characters",
                tree.shown(file)
            ))
        })
        .collect()
}
