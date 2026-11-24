//! An agent's answer as a chat shows it. Agents write Markdown
//! (CommonMark, with tables and strikethrough); each chat formats text
//! its own way. [`Document::parse`] reads the Markdown once into a
//! [`Document`] of blocks and spans, [`pieces`] cuts it into messages no
//! longer than the chat takes, never inside a code block or a table's
//! line, and the connection's [`Dialect`] renders each piece: Telegram's
//! HTML, Slack's mrkdwn, Discord's Markdown, or Signal's text with
//! styles ([`styled`]).

mod fit;
mod markup;
mod parse;
mod styled;
#[cfg(test)]
mod tests;

use serde::Deserialize;

pub use markup::{Markup, Quoted, markup, unfenced};
pub use styled::{Style, Styled, Styling, styled};

/// An answer's Markdown, parsed: its blocks, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

impl Document {
    /// `markdown` as CommonMark reads it, with tables, strikethrough and
    /// task lists. Raw HTML stays text, shown as written; an image is a
    /// link to it.
    pub fn parse(markdown: &str) -> Self {
        Self {
            blocks: parse::blocks(markdown),
        }
    }
}

/// One block of a [`Document`].
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Paragraph(Vec<Span>),
    /// A heading, its level 1 to 6.
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    /// Code, shown as written, in `language` when the fence names one.
    Code {
        language: Option<String>,
        text: String,
    },
    /// A list: numbered from `start`, or bulleted when there's none.
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Quote(Vec<Block>),
    Table(Table),
    /// A thematic break (`---`).
    Rule,
}

/// A table: its head's cells, then each row's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub head: Vec<Vec<Span>>,
    pub rows: Vec<Vec<Vec<Span>>>,
}

impl Table {
    /// The table as aligned text, for a monospace font: each column as
    /// wide as its widest cell, the head ruled off from the rows.
    pub fn monospace(&self) -> String {
        let cell = |spans: &Vec<Span>| plain(spans).replace('\n', " ");
        let lines: Vec<Vec<String>> = std::iter::once(&self.head)
            .chain(&self.rows)
            .map(|row| row.iter().map(cell).collect())
            .collect();
        let columns = lines.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<usize> = (0..columns)
            .map(|c| {
                lines
                    .iter()
                    .filter_map(|line| line.get(c))
                    .map(|text| text.chars().count())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let row = |line: &Vec<String>| {
            let cells: Vec<String> = (0..columns)
                .map(|c| {
                    let text = line.get(c).map_or("", String::as_str);
                    let pad = widths[c] - text.chars().count();
                    format!("{text}{}", " ".repeat(pad))
                })
                .collect();
            cells.join(" | ").trim_end().to_string()
        };
        let mut out = vec![row(&lines[0])];
        let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
        out.push(rule.join("-+-"));
        out.extend(lines[1..].iter().map(row));
        out.join("\n")
    }
}

/// Text marked the same way throughout: bold, a link, or none.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    /// Its marks, the outermost first.
    pub marks: Vec<Mark>,
}

/// What a span is marked as.
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Bold,
    Italic,
    Strike,
    /// Inline code: the innermost mark, holding no other.
    Code,
    /// A link to its URL, as written: [`web_url`] says whether a chat may
    /// link it.
    Link(String),
}

/// Spans as a tree again, for a renderer that opens and closes marks:
/// adjacent spans sharing a mark are one node of it.
#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Marked(Mark, Vec<Inline>),
}

/// `spans` as a tree of [`Inline`]s.
pub fn inlines(spans: &[Span]) -> Vec<Inline> {
    nest(spans, 0)
}

fn nest(spans: &[Span], depth: usize) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < spans.len() {
        match spans[i].marks.get(depth) {
            None => {
                out.push(Inline::Text(spans[i].text.clone()));
                i += 1;
            }
            Some(mark) => {
                let end = spans[i..]
                    .iter()
                    .position(|s| s.marks.get(depth) != Some(mark))
                    .map_or(spans.len(), |n| i + n);
                out.push(Inline::Marked(
                    mark.clone(),
                    nest(&spans[i..end], depth + 1),
                ));
                i = end;
            }
        }
    }
    out
}

/// The text of `spans`, unmarked.
pub fn plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

/// The text of `inlines`, unmarked.
pub fn plain_inlines(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Text(text) => text.clone(),
            Inline::Marked(_, inner) => plain_inlines(inner),
        })
        .collect()
}

/// What every chat may link, as `rich.toml` says.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Links {
    schemes: Vec<String>,
    encoded: String,
}

static LINKS: std::sync::LazyLock<Links> = std::sync::LazyLock::new(|| {
    link_files::schema::parse(include_str!("rich.toml")).expect("rich.toml reads")
});

/// `url` as a chat may link it: its scheme one `rich.toml` names, it
/// holding no control character, and each character that could end the
/// link early in a chat's markup percent-encoded. Else none, and the
/// link is shown as text.
pub fn web_url(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once(':')?;
    let known = LINKS.schemes.iter().any(|s| s.eq_ignore_ascii_case(scheme));
    if !known || rest.is_empty() || url.chars().any(char::is_control) {
        return None;
    }
    Some(
        url.chars()
            .map(|c| match LINKS.encoded.contains(c) {
                true => format!("%{:02X}", c as u32),
                false => c.to_string(),
            })
            .collect(),
    )
}

/// What a chat takes in one message: its `limit`, as it `counted`s.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub limit: usize,
    pub counted: Counted,
}

/// How a chat counts a message's length.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Counted {
    /// In characters (Unicode scalar values).
    Chars,
    /// In UTF-16 code units: an emoji outside the Basic Multilingual
    /// Plane counts two.
    Utf16,
}

impl Counted {
    /// `text`'s length as counted.
    pub fn count(self, text: &str) -> usize {
        match self {
            Self::Chars => text.chars().count(),
            Self::Utf16 => text.encode_utf16().count(),
        }
    }
}

/// How one chat formats text: its renderer, and what one message takes.
pub trait Dialect {
    /// A rendered message: its text, and anything sent beside it.
    type Out: Clone + std::fmt::Debug + PartialEq;
    /// `blocks`, one message's, as the chat takes them.
    fn render(&self, blocks: &[Block]) -> Self::Out;
    /// The text of `out` whose length the chat limits.
    fn text<'a>(&self, out: &'a Self::Out) -> &'a str;
    /// What one message takes.
    fn channel(&self) -> &Channel;
}

/// One message of an answer: its blocks, and them rendered.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece<O> {
    pub blocks: Vec<Block>,
    pub out: O,
}

/// `markdown` as the messages `dialect`'s chat shows it, in order, each
/// within its limit: whole blocks together while they fit, and a block
/// longer than one message cut between its lines, words or items (a code
/// block closed and opened again, a table a code block of its lines). A
/// piece is the same however much text follows it once its blocks are
/// complete, so an answer shown while it's written edits mostly its
/// last. None for an answer of no text.
pub fn pieces<D: Dialect>(markdown: &str, dialect: &D) -> Vec<Piece<D::Out>> {
    fit::pieces(Document::parse(markdown), dialect)
}
