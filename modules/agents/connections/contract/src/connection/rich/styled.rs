//! Rendering for a chat that takes plain text and the ranges of it
//! styled beside it (Signal), and for any chat's plain text.

use super::markup::Lines;
use super::{Block, Inline, Mark, inlines, plain_inlines};

/// A style a range of text is shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Bold,
    Italic,
    Strike,
    Monospace,
}

/// A range of [`Styled::text`] shown in `style`: its start and end, in
/// bytes of the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Styling {
    pub start: usize,
    pub end: usize,
    pub style: Style,
}

/// Plain text and the ranges of it styled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Styled {
    pub text: String,
    pub styles: Vec<Styling>,
}

impl Styled {
    /// Each style's start and length counted in UTF-16 code units, as
    /// Signal counts them.
    pub fn utf16(&self) -> Vec<(usize, usize, Style)> {
        let units = |bytes: usize| self.text[..bytes].encode_utf16().count();
        self.styles
            .iter()
            .map(|s| {
                let start = units(s.start);
                (start, units(s.end) - start, s.style)
            })
            .collect()
    }
}

/// `blocks` as plain text: a heading bold, code and tables monospace, a
/// link's URL after its text (when they differ), a list's items after
/// bullets and a quote's lines after `> `; each mark a style.
pub fn styled(blocks: &[Block]) -> Styled {
    let mut writer = Writer::default();
    writer.blocks(blocks, "\n\n");
    writer.styled.text = writer.lines.out;
    writer.styled.styles.retain(|s| s.start < s.end);
    writer.styled
}

#[derive(Default)]
struct Writer {
    lines: Lines,
    styled: Styled,
}

impl Writer {
    fn blocks(&mut self, blocks: &[Block], between: &str) {
        for (i, block) in blocks.iter().enumerate() {
            if i > 0 {
                self.lines.write(between);
            }
            match block {
                Block::Paragraph(spans) => self.inlines(&inlines(spans)),
                Block::Heading { spans, .. } => {
                    self.styled_with(Style::Bold, |w| w.inlines(&inlines(spans)))
                }
                Block::Code { text, .. } => self.styled_with(Style::Monospace, |w| w.write(text)),
                Block::Table(table) => {
                    let text = table.monospace();
                    self.styled_with(Style::Monospace, |w| w.write(&text))
                }
                Block::Rule => self.write("———"),
                Block::Quote(inner) => {
                    self.lines.indent("> ");
                    self.blocks(inner, "\n\n");
                    self.lines.outdent();
                }
                Block::List { start, items } => {
                    for (n, item) in items.iter().enumerate() {
                        if n > 0 {
                            self.lines.write("\n");
                        }
                        let bullet = match start {
                            Some(s) => format!("{}. ", s.saturating_add(n as u64)),
                            None => "• ".to_string(),
                        };
                        self.lines.item(&bullet);
                        self.blocks(item, "\n");
                        self.lines.outdent();
                    }
                }
            }
        }
    }

    fn write(&mut self, text: &str) {
        self.lines.write(text);
    }

    /// What `write` writes, styled `style`.
    fn styled_with(&mut self, style: Style, write: impl FnOnce(&mut Self)) {
        let start = self.lines.at();
        write(self);
        let end = self.lines.out.len();
        self.styled.styles.push(Styling { start, end, style });
    }

    fn inlines(&mut self, nodes: &[Inline]) {
        for node in nodes {
            match node {
                Inline::Text(text) => self.write(text),
                Inline::Marked(Mark::Link(url), inner) => {
                    let plain = plain_inlines(inner);
                    match plain.trim().is_empty() {
                        true => self.write(url),
                        false => {
                            self.inlines(inner);
                            if plain != *url {
                                self.write(&format!(" ({url})"));
                            }
                        }
                    }
                }
                Inline::Marked(mark, inner) => {
                    let style = match mark {
                        Mark::Bold => Style::Bold,
                        Mark::Italic => Style::Italic,
                        Mark::Strike => Style::Strike,
                        Mark::Code | Mark::Link(_) => Style::Monospace,
                    };
                    self.styled_with(style, |w| w.inlines(inner));
                }
            }
        }
    }
}
