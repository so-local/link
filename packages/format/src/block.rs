//! Blocks: what a command writes to [`Out`]. Each is `#[non_exhaustive]`
//! and built by its builder, so a field added later moves no caller.
//!
//! [`Out`]: crate::Out

use serde::Serialize;

use crate::{Command, Foreign, Path, Phrase, Prose, Report, Sentence, Value};

/// Everything a command can write.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Block {
    Success(Success),
    Fields(Fields),
    Table(Table),
    List(List),
    Tree(Tree),
    Text(Text),
    Section(Section),
    Tail(Tail),
    Next(Next),
    Report(Report),
    Progress(Progress),
    Note(Note),
    Verbatim(Verbatim),
    Pairing(Pairing),
}

/// Something that happened along the way, worth a line: `Tagged your box
/// qa-1 as yours in its cloud.` About the run, so on stderr; plain.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Note(pub Sentence);

/// The outcome a command is for, and what changed where.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Success {
    pub done: Sentence,
    pub changed: Vec<Change>,
    /// It went as far as it can: the rest waits on the person, who's
    /// told what. Shown with the waiting mark, never a failure's.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub waiting: bool,
}

/// What changed and where it's kept: `settings`, `~/Link/Safe/settings.toml`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Change {
    pub what: Phrase,
    pub kept: Value,
}

impl Success {
    pub fn new(done: Sentence) -> Self {
        Success {
            done,
            changed: Vec::new(),
            waiting: false,
        }
    }

    pub fn changed(mut self, what: impl Into<Phrase>, kept: impl Into<Value>) -> Self {
        self.changed.push(Change {
            what: what.into(),
            kept: kept.into(),
        });
        self
    }
}

/// Labels and values, one value perhaps the key, shown in bold.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct Fields {
    pub rows: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Field {
    pub label: Phrase,
    pub value: Value,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub key: bool,
}

impl Fields {
    pub fn new() -> Self {
        Fields::default()
    }

    #[track_caller]
    pub fn row(mut self, label: impl Into<Phrase>, value: impl Into<Value>) -> Self {
        self.rows.push(Field {
            label: label.into(),
            value: value.into(),
            key: false,
        });
        self
    }

    /// The one value that matters most: in bold. At most one a block.
    #[track_caller]
    pub fn key(mut self, label: impl Into<Phrase>, value: impl Into<Value>) -> Self {
        if cfg!(debug_assertions) {
            assert!(
                !self.rows.iter().any(|r| r.key),
                "fields hold at most one key value"
            );
        }
        self.rows.push(Field {
            label: label.into(),
            value: value.into(),
            key: true,
        });
        self
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Typed columns and rows, and what an empty one says, so an empty list
/// is never a bare header.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Value>>,
    pub empty: Sentence,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Column {
    /// Written as a word (`Name`); shown in capitals.
    pub header: String,
    pub align: Align,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    /// Numbers and sizes.
    Right,
}

impl Table {
    #[track_caller]
    pub fn new<const N: usize>(headers: [&str; N], empty: Sentence) -> Self {
        let columns = headers
            .iter()
            .map(|h| {
                crate::words::check_literal(h);
                Column {
                    header: h.to_string(),
                    align: Align::Left,
                }
            })
            .collect();
        Table {
            columns,
            rows: Vec::new(),
            empty,
        }
    }

    /// Column `i` (from 0) lines up on the right.
    pub fn right(mut self, i: usize) -> Self {
        self.columns[i].align = Align::Right;
        self
    }

    #[track_caller]
    pub fn row<V: Into<Value>>(&mut self, cells: impl IntoIterator<Item = V>) {
        let row: Vec<Value> = cells.into_iter().map(Into::into).collect();
        if cfg!(debug_assertions) {
            assert_eq!(row.len(), self.columns.len(), "a row has a cell a column");
        }
        self.rows.push(row);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Parallel items, each on its own bullet, and what an empty list says.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct List {
    pub items: Vec<ListItem>,
    pub empty: Sentence,
}

/// A list holds either a typed value or a paragraph of explanation.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ListItem {
    Value(Value),
    Text(Prose),
}

impl List {
    pub fn new(empty: Sentence) -> Self {
        Self {
            items: Vec::new(),
            empty,
        }
    }

    pub fn item(mut self, item: impl Into<Prose>) -> Self {
        self.items.push(ListItem::Text(item.into()));
        self
    }

    pub fn value(mut self, value: impl Into<Value>) -> Self {
        self.items.push(ListItem::Value(value.into()));
        self
    }
}

/// Rows in a tree: a bucket's folders, each node's cells and its children.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Tree {
    pub roots: Vec<Node>,
    pub empty: Sentence,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Node {
    pub cells: Vec<Value>,
    pub children: Vec<Node>,
}

impl Tree {
    pub fn new(empty: Sentence) -> Self {
        Tree {
            roots: Vec::new(),
            empty,
        }
    }

    pub fn root(mut self, node: Node) -> Self {
        self.roots.push(node);
        self
    }
}

impl Node {
    pub fn new<V: Into<Value>>(cells: impl IntoIterator<Item = V>) -> Self {
        Node {
            cells: cells.into_iter().map(Into::into).collect(),
            children: Vec::new(),
        }
    }

    pub fn child(mut self, node: Node) -> Self {
        self.children.push(node);
        self
    }
}

/// A plain paragraph, in the docs' voice.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Text(pub Prose);

/// A heading over blocks of its own: a group of fields, a tool's grants.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Section {
    pub heading: Phrase,
    pub blocks: Vec<Block>,
}

impl Section {
    pub fn new(heading: impl Into<Phrase>) -> Self {
        Section {
            heading: heading.into(),
            blocks: Vec::new(),
        }
    }

    pub fn with(mut self, block: impl Into<Block>) -> Self {
        self.blocks.push(block.into());
        self
    }
}

/// Another program's last lines, and where the rest is.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Tail {
    pub lines: Vec<Foreign>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<Path>,
    /// What none says: `No log yet.`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty: Option<Sentence>,
}

impl Tail {
    /// The last `n` lines of `text`.
    pub fn last(text: &str, n: usize) -> Self {
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let from = lines.len().saturating_sub(n);
        Tail {
            lines: lines[from..].iter().map(|l| Foreign::new(l)).collect(),
            log: None,
            empty: None,
        }
    }

    pub fn log(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.log = Some(Path::new(path));
        self
    }

    pub fn empty(mut self, says: Sentence) -> Self {
        self.empty = Some(says);
        self
    }
}

/// Lines to copy as they are, such as entries to paste into a spec:
/// never wrapped or word-checked, each perhaps with a `# comment`, and
/// perhaps a lead-in saying what they are, shown ending in a colon.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Verbatim {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead: Option<Phrase>,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Line {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl Verbatim {
    pub fn new() -> Self {
        Verbatim::default()
    }

    /// What the lines are, before them: `Add them` shows as `Add them:`.
    #[track_caller]
    pub fn lead(mut self, lead: impl Into<Phrase>) -> Self {
        self.lead = Some(lead.into());
        self
    }

    pub fn line(mut self, text: impl Into<String>) -> Self {
        self.lines.push(Line {
            text: crate::clean(&text.into()),
            comment: None,
        });
        self
    }

    /// A line and what it is, after a `#`: `"python3",  # an interpreter`.
    pub fn commented(mut self, text: impl Into<String>, comment: impl Into<String>) -> Self {
        self.lines.push(Line {
            text: crate::clean(&text.into()),
            comment: Some(crate::clean(&comment.into())),
        });
        self
    }
}

/// A code a person types elsewhere to pair, such as in a chat, and when
/// it expires: shown, not asked, so written with [`Out::now`].
///
/// [`Out::now`]: crate::Out::now
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Pairing {
    /// Where and how to use it: `In the team chat, mention @link-bot with
    /// this code.`
    pub how: Sentence,
    pub code: String,
    pub expires: crate::Time,
}

impl Pairing {
    pub fn new(how: Sentence, code: impl Into<String>, expires: std::time::SystemTime) -> Self {
        Pairing {
            how,
            code: crate::clean(&code.into()),
            expires: crate::Time(expires),
        }
    }
}

/// What to do next: each a command and why, the likeliest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Next(pub Vec<Suggestion>);

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Suggestion {
    pub command: Command,
    pub why: Phrase,
}

impl Next {
    #[track_caller]
    pub fn push(&mut self, command: Command, why: impl Into<Phrase>) {
        self.0.push(Suggestion {
            command,
            why: why.into(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A step of a counted flow, as `--json` records it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Progress {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub of: Option<usize>,
    pub what: Phrase,
}

impl Progress {
    pub fn new(what: impl Into<Phrase>) -> Self {
        Progress {
            step: None,
            of: None,
            what: what.into(),
        }
    }

    /// Step `step` of `of`: `[2/3]`.
    pub fn of(mut self, step: usize, of: usize) -> Self {
        self.step = Some(step);
        self.of = Some(of);
        self
    }
}

macro_rules! block_from {
    ($($t:ident),*) => {$(
        impl From<$t> for Block {
            fn from(b: $t) -> Self { Block::$t(b) }
        }
    )*};
}
block_from!(
    Success, Fields, Table, List, Tree, Text, Section, Tail, Next, Report, Progress, Note,
    Verbatim, Pairing
);

impl From<Prose> for Block {
    fn from(p: Prose) -> Self {
        Block::Text(Text(p))
    }
}

impl Block {
    /// Whether it's the result, for stdout, as opposed to about the run.
    pub(crate) fn is_result(&self) -> bool {
        matches!(
            self,
            Block::Success(_)
                | Block::Fields(_)
                | Block::Table(_)
                | Block::List(_)
                | Block::Tree(_)
                | Block::Text(_)
                | Block::Section(_)
                | Block::Tail(_)
                | Block::Verbatim(_)
        )
    }
}
