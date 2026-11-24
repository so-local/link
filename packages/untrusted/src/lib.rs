//! The one marker around third-party text a tool hands a model: a page,
//! a file, a document, a screenshot's caption, a diagnostic or a
//! program's output. Each is fenced between an opening line that says
//! what it is and where it's from, and a closing line carrying the same
//! random ID ([`link_id::candidate`]), so the text can't close the fence
//! early and speak as the tool.

use std::sync::LazyLock;

use serde::Deserialize;

pub use link_id::NoRandomness;

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(unit);

/// The fence's words, as `marker.toml` sets them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    tag: String,
    note: String,
    name_max: usize,
}

static MARKER: LazyLock<Marker> =
    LazyLock::new(|| toml::from_str(include_str!("marker.toml")).expect("link-untrusted's marker"));

/// The note every opening line carries: that the text is data, and that
/// the model follows and repeats none of its instructions.
pub fn note() -> &'static str {
    &MARKER.note
}

/// What a fenced text is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A web page, or a part of one.
    Page,
    /// A search engine's results.
    Search,
    /// A file's contents.
    File,
    /// A document's text, from a PDF or a Word file.
    Document,
    /// What a screenshot shows: an image follows the fence's note.
    Screenshot,
    /// A language server's diagnostics.
    Diagnostics,
    /// A program's output or errors.
    Output,
    /// A page's dialog, title or other text the browser shows.
    Dialog,
}

impl Kind {
    /// Its word in the opening line.
    pub fn word(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Search => "search",
            Self::File => "file",
            Self::Document => "document",
            Self::Screenshot => "screenshot",
            Self::Diagnostics => "diagnostics",
            Self::Output => "output",
            Self::Dialog => "dialog",
        }
    }
}

/// Where a fenced text is from: its kind and a name, a URL or a path.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub kind: Kind,
    pub name: &'a str,
}

/// One fence: its random ID, drawn once, for one text or several parts
/// of one answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fence {
    id: String,
}

impl Fence {
    /// A fence with a newly drawn ID.
    pub fn new() -> Result<Self, NoRandomness> {
        Ok(Self {
            id: link_id::candidate()?,
        })
    }

    /// Its ID, which only its closing line carries.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Its opening line, ended by a newline.
    pub fn open(&self, source: Source<'_>) -> String {
        format!(
            "<{tag} kind=\"{}\" from=\"{}\" id=\"{}\" note=\"{}\">\n",
            source.kind.word(),
            shown(source.name),
            self.id,
            MARKER.note,
            tag = MARKER.tag,
        )
    }

    /// Its closing line, with no newline after it.
    pub fn close(&self) -> String {
        format!("</{} id=\"{}\">", MARKER.tag, self.id)
    }

    /// `text` fenced: the opening line, the text with its control
    /// characters (all but newlines and tabs) dropped, a newline, and the
    /// closing line, so the text reads back as it was ([`fenced`]).
    pub fn wrap(&self, source: Source<'_>, text: &str) -> String {
        let mut out = self.open(source);
        out.extend(text.chars().filter(|c| kept(*c)));
        out.push('\n');
        out += &self.close();
        out
    }
}

/// `text` fenced as from `source`, in a fence of its own.
pub fn fence(source: Source<'_>, text: &str) -> Result<String, NoRandomness> {
    Ok(Fence::new()?.wrap(source, text))
}

/// A fenced text as a reader of a tool's answer finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fenced<'a> {
    /// Its kind's word (`page`, `file`).
    pub kind: &'a str,
    /// Its source's name, as the opening line shows it.
    pub from: &'a str,
    /// Its text, as it was fenced: the newline the fence adds before its
    /// closing line taken off.
    pub text: &'a str,
}

/// The first fence in `text`, read back: its kind, its source and its
/// text up to the closing line with its own ID. None when `text` has no
/// whole fence.
pub fn fenced(text: &str) -> Option<Fenced<'_>> {
    first(text).map(|(found, _)| found)
}

/// `text` as a reader sees it plainly: each fence, from its opening line
/// to its own closing line, given back as the text it holds.
pub fn unfenced(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some((found, span)) = first(rest) {
        out.push_str(&rest[..span.start]);
        out.push_str(found.text);
        rest = &rest[span.end..];
    }
    out + rest
}

/// The first whole fence in `text` and the bytes it spans.
fn first(text: &str) -> Option<(Fenced<'_>, std::ops::Range<usize>)> {
    let open = text.find(&format!("<{} kind=\"", MARKER.tag))?;
    let line_end = open + text[open..].find('\n')?;
    let line = &text[open..line_end];
    // A name never holds a double quote ([`shown`]), so each attribute
    // ends at the first one after it.
    let attr = |name: &str| {
        let key = format!(" {name}=\"");
        let at = line.find(&key)? + key.len();
        Some(&line[at..at + line[at..].find('"')?])
    };
    let (kind, from, id) = (attr("kind")?, attr("from")?, attr("id")?);
    let close = format!("\n</{} id=\"{id}\">", MARKER.tag);
    let end = line_end + text[line_end..].find(&close)?;
    let found = Fenced {
        kind,
        from,
        text: text.get(line_end + 1..end).unwrap_or_default(),
    };
    Some((found, open..end + close.len()))
}

/// A character kept inside the fence: none that moves the cursor or
/// colours a terminal, which a model or a person reading a log would be
/// shown as if the tool had written it.
fn kept(c: char) -> bool {
    c == '\n' || c == '\t' || !c.is_control()
}

/// A source's name as the opening line shows it: on one line, with no
/// quote to end the attribute, cut at `name_max` characters.
fn shown(name: &str) -> String {
    let mut out: String = name
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if c == '"' { '\'' } else { c })
        .take(MARKER.name_max)
        .collect();
    if name.chars().count() > MARKER.name_max {
        out.push('…');
    }
    out
}
