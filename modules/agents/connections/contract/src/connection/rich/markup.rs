//! Rendering for a chat that marks text with characters or tags: each
//! chat says how it writes a mark or a block ([`Markup`]), and
//! [`markup`] lays out the blocks, lists and quotes the same for all.

use super::{Block, Inline, Mark, Span, inlines, plain_inlines, web_url};

/// How a chat writes formatted text: its connection's half of
/// [`markup`].
pub trait Markup {
    /// `text` escaped, so the chat shows it as written.
    fn text(&self, text: &str) -> String;
    /// What opens and closes `mark`: bold, italic or strikethrough.
    fn mark(&self, mark: &Mark) -> (&str, &str);
    /// Inline code, as written.
    fn code(&self, text: &str) -> String;
    /// A link to `url`, already safe to link ([`web_url`]), shown as
    /// `shown`, rendered, whose unmarked text is `plain`.
    fn link(&self, url: &str, shown: &str, plain: &str) -> String;
    /// A heading of `level`, its text `shown`, rendered, whose unmarked
    /// text is `plain`.
    fn heading(&self, level: u8, shown: &str, plain: &str) -> String;
    /// A code block in `language`, its `text` as written.
    fn code_block(&self, language: Option<&str>, text: &str) -> String;
    /// How a quote is shown.
    fn quote(&self) -> Quoted;
    /// What starts a list's item: `number`ed, or a bullet.
    fn bullet(&self, number: Option<u64>) -> String;
    /// A thematic break.
    fn rule(&self) -> String;
}

/// How a chat shows a quote.
pub enum Quoted {
    /// Between an opening and a closing tag.
    Wrapped(String, String),
    /// Each line after a prefix (`> `).
    Prefixed(String),
}

/// `text`, a code block's, with each `fence` in it kept from ending the
/// block early: a zero-width space before its last character.
pub fn unfenced(text: &str, fence: &str) -> String {
    let Some(last) = fence.chars().last() else {
        return text.to_string();
    };
    let open = &fence[..fence.len() - last.len_utf8()];
    text.replace(fence, &format!("{open}\u{200b}{last}"))
}

/// `blocks` as `chat` writes them: blocks a blank line apart, a list's
/// items a line each, their lines indented under the bullet.
pub fn markup(chat: &impl Markup, blocks: &[Block]) -> String {
    let mut lines = Lines::default();
    write_blocks(chat, &mut lines, blocks, "\n\n");
    lines.out
}

fn write_blocks(chat: &impl Markup, lines: &mut Lines, blocks: &[Block], between: &str) {
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            lines.write(between);
        }
        match block {
            Block::Paragraph(spans) => lines.write(&inline(chat, spans)),
            Block::Heading { level, spans } => {
                lines.write(&chat.heading(*level, &inline(chat, spans), &super::plain(spans)))
            }
            Block::Code { language, text } => {
                lines.write(&chat.code_block(language.as_deref(), text))
            }
            Block::Table(table) => lines.write(&chat.code_block(None, &table.monospace())),
            Block::Rule => lines.write(&chat.rule()),
            Block::Quote(inner) => match chat.quote() {
                Quoted::Wrapped(open, close) => {
                    lines.write(&open);
                    write_blocks(chat, lines, inner, "\n\n");
                    lines.write(&close);
                }
                Quoted::Prefixed(prefix) => {
                    lines.indent(&prefix);
                    write_blocks(chat, lines, inner, "\n\n");
                    lines.outdent();
                }
            },
            Block::List { start, items } => {
                for (n, item) in items.iter().enumerate() {
                    if n > 0 {
                        lines.write("\n");
                    }
                    let number = start.map(|s| s.saturating_add(n as u64));
                    lines.item(&chat.bullet(number));
                    write_blocks(chat, lines, item, "\n");
                    lines.outdent();
                }
            }
        }
    }
}

/// `spans` as `chat` writes them.
pub(super) fn inline(chat: &impl Markup, spans: &[Span]) -> String {
    write_inlines(chat, &inlines(spans))
}

fn write_inlines(chat: &impl Markup, nodes: &[Inline]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Inline::Text(text) => out.push_str(&chat.text(text)),
            Inline::Marked(Mark::Code, inner) => out.push_str(&chat.code(&plain_inlines(inner))),
            Inline::Marked(Mark::Link(url), inner) => {
                let plain = plain_inlines(inner);
                let shown = match plain.trim().is_empty() {
                    true => chat.text(url),
                    false => write_inlines(chat, inner),
                };
                out.push_str(&match web_url(url) {
                    Some(url) => chat.link(&url, &shown, &plain),
                    None if plain.is_empty() || plain == *url => chat.text(url),
                    None => format!("{shown} ({})", chat.text(url)),
                });
            }
            Inline::Marked(mark, inner) => {
                let (open, close) = chat.mark(mark);
                out.push_str(open);
                out.push_str(&write_inlines(chat, inner));
                out.push_str(close);
            }
        }
    }
    out
}

/// Text written a line at a time, each line after the prefixes of the
/// lists and quotes it's in.
#[derive(Default)]
pub(super) struct Lines {
    pub(super) out: String,
    prefixes: Vec<String>,
    /// At a line's start, its prefix not yet written.
    begun: bool,
}

impl Lines {
    /// Writes `text`, each line of it after the prefixes; an empty line
    /// gets them without their trailing space.
    pub(super) fn write(&mut self, text: &str) {
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                if !self.begun {
                    let prefix = self.prefix();
                    self.out.push_str(prefix.trim_end());
                }
                self.out.push('\n');
                self.begun = false;
            }
            if !line.is_empty() {
                self.begin();
                self.out.push_str(line);
            }
        }
    }

    /// Where the next character written will be, its line's prefix
    /// written first.
    pub(super) fn at(&mut self) -> usize {
        self.begin();
        self.out.len()
    }

    fn begin(&mut self) {
        if !self.begun {
            let prefix = self.prefix();
            self.out.push_str(&prefix);
            self.begun = true;
        }
    }

    fn prefix(&self) -> String {
        self.prefixes.concat()
    }

    /// Starts a list's item with `bullet`, its next lines indented under
    /// it until [`Lines::outdent`].
    pub(super) fn item(&mut self, bullet: &str) {
        self.begin();
        self.out.push_str(bullet);
        self.prefixes.push(" ".repeat(bullet.chars().count()));
    }

    /// Writes each line from here after `prefix` too, until
    /// [`Lines::outdent`].
    pub(super) fn indent(&mut self, prefix: &str) {
        self.prefixes.push(prefix.to_string());
    }

    pub(super) fn outdent(&mut self) {
        self.prefixes.pop();
    }
}
