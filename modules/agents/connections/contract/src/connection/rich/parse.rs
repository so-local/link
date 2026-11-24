//! Markdown to [`Block`]s, by `pulldown-cmark`'s events.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::{Block, Mark, Span, Table};

/// A block being read: a container of blocks, or one of text.
enum Open {
    /// The document, a quote or a list's item.
    Blocks(Vec<Block>),
    Quote(Vec<Block>),
    Item(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    /// A paragraph, opened by Markdown or by text in a tight list's item.
    Paragraph(Vec<Span>),
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Table {
        table: Table,
        row: Vec<Vec<Span>>,
        cell: Vec<Span>,
        head: bool,
    },
}

struct Reader {
    open: Vec<Open>,
    marks: Vec<Mark>,
}

pub(super) fn blocks(markdown: &str) -> Vec<Block> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut reader = Reader {
        open: vec![Open::Blocks(Vec::new())],
        marks: Vec::new(),
    };
    for event in Parser::new_ext(markdown, options) {
        reader.read(event);
    }
    // An answer cut short closes what it opened.
    while reader.open.len() > 1 {
        reader.close();
    }
    match reader.open.pop() {
        Some(Open::Blocks(blocks)) => blocks,
        _ => Vec::new(),
    }
}

impl Reader {
    fn read(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                self.marks.push(Mark::Code);
                self.text(&code);
                self.marks.pop();
            }
            Event::InlineMath(text) | Event::DisplayMath(text) => self.text(&text),
            // Raw HTML is shown as written, never interpreted.
            Event::Html(html) | Event::InlineHtml(html) => self.text(&html),
            Event::FootnoteReference(name) => self.text(&format!("[^{name}]")),
            Event::SoftBreak | Event::HardBreak => self.text("\n"),
            Event::Rule => {
                self.end_paragraph();
                self.push(Block::Rule);
            }
            Event::TaskListMarker(done) => self.text(if done { "☑ " } else { "☐ " }),
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                self.end_paragraph();
                self.open.push(Open::Paragraph(Vec::new()));
            }
            Tag::Heading { level, .. } => {
                self.end_paragraph();
                self.open.push(Open::Heading {
                    level: level as u8,
                    spans: Vec::new(),
                });
            }
            Tag::BlockQuote(_) => {
                self.end_paragraph();
                self.open.push(Open::Quote(Vec::new()));
            }
            Tag::CodeBlock(kind) => {
                self.end_paragraph();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .map(str::to_string)
                        .filter(|l| !l.is_empty()),
                    CodeBlockKind::Indented => None,
                };
                self.open.push(Open::Code {
                    language,
                    text: String::new(),
                });
            }
            Tag::List(start) => {
                self.end_paragraph();
                self.open.push(Open::List {
                    start,
                    items: Vec::new(),
                });
            }
            Tag::Item => self.open.push(Open::Item(Vec::new())),
            Tag::Table(_) => {
                self.end_paragraph();
                self.open.push(Open::Table {
                    table: Table::default(),
                    row: Vec::new(),
                    cell: Vec::new(),
                    head: false,
                });
            }
            Tag::TableHead => self.in_table(|_, _, head| *head = true),
            Tag::Emphasis => self.marks.push(Mark::Italic),
            Tag::Strong => self.marks.push(Mark::Bold),
            Tag::Strikethrough => self.marks.push(Mark::Strike),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.marks.push(Mark::Link(dest_url.to_string()));
            }
            // Read as the blocks or text they hold.
            Tag::HtmlBlock
            | Tag::TableRow
            | Tag::TableCell
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::Table => {
                self.close()
            }
            TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item => {
                self.end_paragraph();
                self.close();
            }
            TagEnd::TableHead => self.in_table(|table, row, head| {
                table.head = std::mem::take(row);
                *head = false;
            }),
            TagEnd::TableRow => self.in_table(|table, row, _| {
                table.rows.push(std::mem::take(row));
            }),
            TagEnd::TableCell => {
                if let Some(Open::Table { row, cell, .. }) = self.open.last_mut() {
                    row.push(trimmed(std::mem::take(cell)));
                }
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Link
            | TagEnd::Image => {
                self.marks.pop();
            }
            TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => {}
        }
    }

    fn in_table(&mut self, change: impl FnOnce(&mut Table, &mut Vec<Vec<Span>>, &mut bool)) {
        if let Some(Open::Table {
            table, row, head, ..
        }) = self.open.last_mut()
        {
            change(table, row, head);
        }
    }

    /// `text`, marked as the marks open say, where text goes now.
    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(Open::Code { text: code, .. }) = self.open.last_mut() {
            code.push_str(text);
            return;
        }
        if matches!(
            self.open.last(),
            Some(Open::Blocks(_) | Open::Quote(_) | Open::Item(_) | Open::List { .. })
        ) {
            self.open.push(Open::Paragraph(Vec::new()));
        }
        let spans = match self.open.last_mut() {
            Some(Open::Paragraph(spans) | Open::Heading { spans, .. }) => spans,
            Some(Open::Table { cell, .. }) => cell,
            _ => return,
        };
        add(spans, text, &self.marks);
    }

    /// Closes a paragraph a tight list's text opened, before a block.
    fn end_paragraph(&mut self) {
        if matches!(self.open.last(), Some(Open::Paragraph(_))) {
            self.close();
        }
    }

    /// Closes the innermost open block into the one holding it.
    fn close(&mut self) {
        let Some(open) = self.open.pop() else { return };
        let block = match open {
            Open::Paragraph(spans) => Some(Block::Paragraph(trimmed(spans))),
            Open::Heading { level, spans } => Some(Block::Heading {
                level,
                spans: trimmed(spans),
            }),
            Open::Code { language, text } => Some(Block::Code {
                language,
                text: text.strip_suffix('\n').unwrap_or(&text).to_string(),
            }),
            Open::Quote(blocks) => Some(Block::Quote(blocks)),
            Open::List { start, items } => Some(Block::List { start, items }),
            Open::Table { table, .. } => Some(Block::Table(table)),
            Open::Item(blocks) => {
                if let Some(Open::List { items, .. }) = self.open.last_mut() {
                    items.push(blocks);
                }
                None
            }
            Open::Blocks(blocks) => {
                // Never closed but by the end: kept as the document.
                self.open.push(Open::Blocks(blocks));
                None
            }
        };
        if let Some(block) = block {
            self.push(block);
        }
    }

    fn push(&mut self, block: Block) {
        if matches!(&block, Block::Paragraph(spans) if spans.is_empty()) {
            return;
        }
        match self.open.last_mut() {
            Some(Open::Blocks(blocks) | Open::Quote(blocks) | Open::Item(blocks)) => {
                blocks.push(block)
            }
            Some(Open::List { items, .. }) => items.push(vec![block]),
            _ => {}
        }
    }
}

/// `text` added to `spans` with `marks`: into the last span when it's
/// marked the same.
fn add(spans: &mut Vec<Span>, text: &str, marks: &[Mark]) {
    if text.is_empty() {
        return;
    }
    match spans.last_mut() {
        Some(last) if last.marks == marks => last.text.push_str(text),
        _ => spans.push(Span {
            text: text.to_string(),
            marks: marks.to_vec(),
        }),
    }
}

fn emphasis(mark: &Mark) -> bool {
    matches!(mark, Mark::Bold | Mark::Italic | Mark::Strike)
}

/// `spans` with the space at either end of emphasized text left out of
/// it, as a chat marking with characters (`*bold*`) needs: `** a **`
/// would be no emphasis there.
pub(super) fn hoisted(spans: Vec<Span>) -> Vec<Span> {
    // Where emphasis starts or ends at `span`, beside `other`: the
    // outermost such mark's depth.
    let edge = |span: &Span, other: Option<&Span>| {
        (0..span.marks.len()).find(|&d| {
            emphasis(&span.marks[d])
                && other.is_none_or(|o| o.marks.get(..=d) != Some(&span.marks[..=d]))
        })
    };
    let mut out: Vec<Span> = Vec::new();
    for (i, span) in spans.iter().enumerate() {
        if span.marks.contains(&Mark::Code) {
            add(&mut out, &span.text, &span.marks);
            continue;
        }
        let (mut lead, mut trail) = (0, span.text.len());
        let start = edge(span, i.checked_sub(1).map(|p| &spans[p]));
        let end = edge(span, spans.get(i + 1));
        if start.is_some() {
            lead = span.text.len() - span.text.trim_start().len();
        }
        if end.is_some() {
            trail = lead.max(span.text.trim_end().len());
        }
        if let Some(d) = start {
            add(&mut out, &span.text[..lead], &span.marks[..d]);
        }
        add(&mut out, &span.text[lead..trail], &span.marks);
        if let Some(d) = end {
            add(&mut out, &span.text[trail..], &span.marks[..d]);
        }
    }
    out.retain(|s| !s.text.is_empty());
    out
}

/// `spans` without space at their start and end.
fn trimmed(spans: Vec<Span>) -> Vec<Span> {
    let mut spans = hoisted(spans);
    if let Some(first) = spans.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    if let Some(last) = spans.last_mut() {
        last.text = last.text.trim_end().to_string();
    }
    spans.retain(|s| !s.text.is_empty());
    spans
}
