//! A document cut into messages a chat takes.

use super::{Block, Dialect, Document, Piece, Span};

pub(super) fn pieces<D: Dialect>(document: Document, dialect: &D) -> Vec<Piece<D::Out>> {
    let channel = dialect.channel();
    let fits = |blocks: &[Block]| {
        channel.counted.count(dialect.text(&dialect.render(blocks))) <= channel.limit
    };
    let mut fitted = Vec::new();
    for block in document.blocks {
        fit(block, &fits, &mut fitted);
    }
    // Whole blocks together, as many as fit.
    let mut groups: Vec<Vec<Block>> = Vec::new();
    let mut group: Vec<Block> = Vec::new();
    for block in fitted {
        group.push(block);
        if group.len() > 1 && !fits(&group) {
            let last = group.pop().expect("pushed");
            groups.push(std::mem::replace(&mut group, vec![last]));
        }
    }
    if !group.is_empty() {
        groups.push(group);
    }
    groups
        .into_iter()
        .map(|blocks| Piece {
            out: dialect.render(&blocks),
            blocks,
        })
        .collect()
}

/// `block` into `out`, whole when it fits one message, else in parts
/// that each do.
fn fit(block: Block, fits: &impl Fn(&[Block]) -> bool, out: &mut Vec<Block>) {
    if fits(std::slice::from_ref(&block)) {
        out.push(block);
        return;
    }
    match block {
        Block::Paragraph(spans) => {
            for spans in cut_spans(spans, ' ', |s| fits(&[Block::Paragraph(s.to_vec())])) {
                out.push(Block::Paragraph(spans));
            }
        }
        Block::Heading { level, spans } => {
            let heading = |spans: &[Span]| Block::Heading {
                level,
                spans: spans.to_vec(),
            };
            for spans in cut_spans(spans, ' ', |s| fits(&[heading(s)])) {
                out.push(heading(&spans));
            }
        }
        Block::Code { language, text } => {
            let code = |text: &str| Block::Code {
                language: language.clone(),
                text: text.to_string(),
            };
            for text in cut_text(&text, '\n', |t| fits(&[code(t)])) {
                out.push(code(&text));
            }
        }
        Block::Table(table) => fit(
            Block::Code {
                language: None,
                text: table.monospace(),
            },
            fits,
            out,
        ),
        Block::List { start, items } => {
            let list = |first: usize, items: &[Vec<Block>]| Block::List {
                start: start.map(|s| s.saturating_add(first as u64)),
                items: items.to_vec(),
            };
            let mut first = 0;
            while first < items.len() {
                let mut end = first + 1;
                while end < items.len() && fits(&[list(first, &items[first..=end])]) {
                    end += 1;
                }
                match fits(&[list(first, &items[first..end])]) {
                    true => out.push(list(first, &items[first..end])),
                    // An item longer than a message: its blocks alone.
                    false => {
                        for block in items[first].clone() {
                            fit(block, fits, out);
                        }
                    }
                }
                first = end;
            }
        }
        Block::Quote(inner) => {
            let mut group: Vec<Block> = Vec::new();
            for block in inner {
                group.push(block);
                if fits(&[Block::Quote(group.clone())]) {
                    continue;
                }
                let last = group.pop().expect("pushed");
                if !group.is_empty() {
                    out.push(Block::Quote(std::mem::take(&mut group)));
                }
                match fits(&[Block::Quote(vec![last.clone()])]) {
                    true => group.push(last),
                    // A block longer than a message: its parts quoted.
                    false => {
                        let mut parts = Vec::new();
                        fit(last, fits, &mut parts);
                        for part in parts {
                            let quoted = Block::Quote(vec![part.clone()]);
                            out.push(match fits(std::slice::from_ref(&quoted)) {
                                true => quoted,
                                false => part,
                            });
                        }
                    }
                }
            }
            if !group.is_empty() {
                out.push(Block::Quote(group));
            }
        }
        Block::Rule => out.push(Block::Rule),
    }
}

/// `text` in parts each `fits`, cut after the last `at` that leaves the
/// part fitting (dropped at the cut), else as far as fits.
fn cut_text(text: &str, at: char, fits: impl Fn(&str) -> bool) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut parts = Vec::new();
    let mut rest = &chars[..];
    while !rest.is_empty() {
        let part = |n: usize| rest[..n].iter().collect::<String>();
        if fits(&part(rest.len())) {
            parts.push(part(rest.len()));
            break;
        }
        let most = most(rest.len(), |n| fits(&part(n)));
        let (end, skip) = match rest[..most].iter().rposition(|c| *c == at) {
            Some(i) if i > 0 => (i, 1),
            _ => (most, 0),
        };
        parts.push(part(end));
        rest = &rest[end + skip..];
    }
    parts
}

/// `spans` in parts each `fits`, cut as [`cut_text`] cuts text.
fn cut_spans(spans: Vec<Span>, at: char, fits: impl Fn(&[Span]) -> bool) -> Vec<Vec<Span>> {
    let mut parts = Vec::new();
    let mut rest = spans;
    loop {
        let length: usize = rest.iter().map(|s| s.text.chars().count()).sum();
        if length == 0 {
            break;
        }
        if fits(&rest) {
            parts.push(rest);
            break;
        }
        let most = most(length, |n| fits(&split_spans(&rest, n).0));
        let text: Vec<char> = rest.iter().flat_map(|s| s.text.chars()).collect();
        let (end, skip) = match text[..most]
            .iter()
            .rposition(|c| c.is_whitespace() || *c == at)
        {
            Some(i) if i > 0 => (i, 1),
            _ => (most, 0),
        };
        let (part, after) = split_spans(&rest, end);
        parts.push(part);
        rest = split_spans(&after, skip).1;
    }
    parts
}

/// `spans` cut after `n` characters: those before, and those after.
fn split_spans(spans: &[Span], n: usize) -> (Vec<Span>, Vec<Span>) {
    let (mut before, mut after) = (Vec::new(), Vec::new());
    let mut left = n;
    for span in spans {
        let count = span.text.chars().count();
        if left >= count {
            before.push(span.clone());
            left -= count;
        } else if left == 0 {
            after.push(span.clone());
        } else {
            let at = span
                .text
                .char_indices()
                .nth(left)
                .map_or(span.text.len(), |(i, _)| i);
            before.push(Span {
                text: span.text[..at].to_string(),
                marks: span.marks.clone(),
            });
            after.push(Span {
                text: span.text[at..].to_string(),
                marks: span.marks.clone(),
            });
            left = 0;
        }
    }
    (before, after)
}

/// The most of `length` (at least 1) that `fits`, by halving: what fits
/// a message grows with what's in it.
fn most(length: usize, fits: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (1, length);
    if !fits(lo) {
        return 1;
    }
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        match fits(mid) {
            true => lo = mid,
            false => hi = mid - 1,
        }
    }
    lo
}
