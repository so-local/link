//! A [`Page`] written as markdown: headings, paragraphs, lists, links,
//! emphasis, code, block quotes and tables, each link whole.

use super::*;
use crate::page::{DOCUMENT, Kind, Page};

/// The tags left out of a page's markdown: what runs, what's drawn, and
/// what's around the content rather than in it.
const SKIPPED: &[&str] = &[
    "head", "script", "style", "noscript", "template", "svg", "canvas", "iframe", "object",
    "embed", "img", "picture", "video", "audio", "source", "nav", "footer", "form", "button",
    "input", "select", "textarea",
];

/// The tags that stand as a block of their own, between blank lines.
const BLOCKS: &[&str] = &[
    "address",
    "article",
    "aside",
    "body",
    "caption",
    "center",
    "dd",
    "details",
    "dialog",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "header",
    "hgroup",
    "html",
    "li",
    "main",
    "p",
    "section",
    "summary",
];

/// `page` as markdown, each link made whole against `base`. Written by
/// recursion, so `page` must nest no deeper than the stack holds.
pub(super) fn write(page: &Page, base: &Url) -> String {
    Writer { page, base }.children(DOCUMENT)
}

/// `page`'s text alone, its spaces made one, what [`write`] leaves out
/// left out here too: walked without recursion, for a page nested too
/// deep to write as markdown.
pub(super) fn text(page: &Page) -> String {
    let mut words = String::new();
    let mut stack = vec![DOCUMENT];
    while let Some(id) = stack.pop() {
        match &page.nodes[id].kind {
            Kind::Text(text) => {
                words.push_str(text);
                words.push(' ');
                continue;
            }
            Kind::Element { name, .. } if SKIPPED.contains(&&*name.local) || page.hidden(id) => {
                continue;
            }
            Kind::Other => continue,
            Kind::Element { .. } | Kind::Document => {}
        }
        // Children last first, so the first is popped first.
        stack.extend(page.nodes[id].children.iter().rev());
    }
    words.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Writer<'a> {
    page: &'a Page,
    base: &'a Url,
}

impl Writer<'_> {
    /// Node `id`'s children, written one after another.
    fn children(&self, id: usize) -> String {
        let mut out = String::new();
        for child in &self.page.nodes[id].children {
            join(&mut out, &self.node(*child));
        }
        out
    }

    /// Node `id` as markdown: a block starts and ends with a blank line,
    /// inline text has its spaces made one.
    fn node(&self, id: usize) -> String {
        let name = match &self.page.nodes[id].kind {
            Kind::Element { name, .. } => &*name.local,
            Kind::Text(text) => return collapse(text),
            Kind::Document => return self.children(id),
            Kind::Other => return String::new(),
        };
        match name {
            _ if SKIPPED.contains(&name) || self.page.hidden(id) => String::new(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let text = squash(&self.children(id));
                match text.is_empty() {
                    true => String::new(),
                    false => block(&format!(
                        "{} {text}",
                        "#".repeat(usize::from(name.as_bytes()[1] - b'0'))
                    )),
                }
            }
            "br" => "\n".into(),
            "hr" => block("---"),
            "strong" | "b" => self.around(id, "**"),
            "em" | "i" => self.around(id, "*"),
            "s" | "del" | "strike" => self.around(id, "~~"),
            "code" | "kbd" | "samp" | "tt" => code_span(collapse(&self.text(id)).trim()),
            "pre" => self.code_block(id),
            "a" => self.link(id),
            "ul" | "ol" => self.list(id, name == "ol"),
            "blockquote" => {
                let inner = compact(&self.children(id));
                let lines = inner.trim_matches([' ', '\n']).lines();
                block(
                    &lines
                        .map(|l| {
                            if l.is_empty() {
                                ">".into()
                            } else {
                                format!("> {l}")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }
            "table" => self.table(id),
            _ if BLOCKS.contains(&name) => block(&self.children(id)),
            _ => self.children(id),
        }
    }

    /// Element `id`'s text, marked on both sides by `mark`, its spaces
    /// kept outside the marks.
    fn around(&self, id: usize, mark: &str) -> String {
        let inner = self.children(id);
        let text = squash(&inner);
        spaced(
            &inner,
            if text.is_empty() {
                text
            } else {
                format!("{mark}{text}{mark}")
            },
        )
    }

    /// A link, made whole against the page; one that isn't the web's
    /// (`javascript:`) is its text alone, and one with no text is left out.
    fn link(&self, id: usize) -> String {
        let inner = self.children(id);
        let text = squash(&inner);
        let whole = self
            .page
            .attr(id, "href")
            .and_then(|href| self.base.join(href.trim()).ok())
            .filter(|u| matches!(u.scheme(), "http" | "https" | "mailto"))
            .map(String::from);
        let made = match whole {
            _ if text.is_empty() => text,
            None => text,
            Some(url) if url.contains(['(', ')', ' ']) => format!("[{text}](<{url}>)"),
            Some(url) => format!("[{text}]({url})"),
        };
        spaced(&inner, made)
    }

    /// A list, each item's marker on its first line and the rest
    /// indented under it.
    fn list(&self, id: usize, ordered: bool) -> String {
        let mut number = match ordered {
            true => self
                .page
                .attr(id, "start")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(1),
            false => 0,
        };
        let mut items = Vec::new();
        for child in &self.page.nodes[id].children {
            let content = match self.page.name(*child) {
                Some("li") => self.children(*child),
                _ => self.node(*child),
            };
            let content = tighten(content.trim_matches([' ', '\n']));
            if content.is_empty() {
                continue;
            }
            let marker = match ordered {
                true => format!("{number}. "),
                false => "- ".into(),
            };
            number += 1;
            let indent = " ".repeat(marker.len());
            let mut item = String::new();
            for (i, line) in content.lines().enumerate() {
                match i {
                    0 => item.push_str(&marker),
                    _ if line.is_empty() => {}
                    _ => item.push_str(&indent),
                }
                item.push_str(line);
                item.push('\n');
            }
            items.push(item);
        }
        block(&items.concat())
    }

    /// A `<pre>` as a fenced code block, in the language its `<code>`'s
    /// class names.
    fn code_block(&self, id: usize) -> String {
        let text = self.text(id);
        let code = text.trim_start_matches('\n').trim_end();
        if code.is_empty() {
            return String::new();
        }
        let language = self.page.nodes[id]
            .children
            .iter()
            .filter(|c| self.page.name(**c) == Some("code"))
            .filter_map(|c| self.page.attr(*c, "class"))
            .flat_map(str::split_whitespace)
            .find_map(|class| {
                class
                    .strip_prefix("language-")
                    .or_else(|| class.strip_prefix("lang-"))
            })
            .unwrap_or_default();
        let fence = '`'.to_string().repeat(longest_run(code, '`').max(2) + 1);
        block(&format!("{fence}{language}\n{code}\n{fence}"))
    }

    /// A table: its first row the header, a cell's `|` escaped.
    fn table(&self, id: usize) -> String {
        let page = self.page;
        let mut caption = String::new();
        let mut rows = Vec::new();
        let mut stack: Vec<usize> = page.nodes[id].children.iter().rev().copied().collect();
        while let Some(node) = stack.pop() {
            match page.name(node) {
                Some("caption") => caption = squash(&self.children(node)),
                Some("thead" | "tbody" | "tfoot") => {
                    stack.extend(page.nodes[node].children.iter().rev())
                }
                Some("tr") => {
                    let cells: Vec<String> = page.nodes[node]
                        .children
                        .iter()
                        .filter(|c| matches!(page.name(**c), Some("td" | "th")))
                        .map(|c| squash(&self.children(*c)).replace('|', "\\|"))
                        .collect();
                    if !cells.is_empty() {
                        rows.push(cells);
                    }
                }
                _ => {}
            }
        }
        let Some(width) = rows.iter().map(Vec::len).max() else {
            return block(&caption);
        };
        let line = |cells: &[String]| {
            let mut line = String::from("|");
            for i in 0..width {
                line.push(' ');
                line.push_str(cells.get(i).map_or("", String::as_str));
                line.push_str(" |");
            }
            line
        };
        let mut lines = vec![line(&rows[0]), line(&vec!["---".to_string(); width])];
        lines.extend(rows[1..].iter().map(|row| line(row)));
        let table = lines.join("\n");
        match caption.is_empty() {
            true => block(&table),
            false => block(&format!("{caption}\n\n{table}")),
        }
    }

    /// Node `id`'s text as it stands, a `<br>` a new line.
    fn text(&self, id: usize) -> String {
        match &self.page.nodes[id].kind {
            Kind::Text(text) => text.clone(),
            _ if self.page.name(id) == Some("br") => "\n".into(),
            _ => self.page.nodes[id]
                .children
                .iter()
                .map(|c| self.text(*c))
                .collect(),
        }
    }
}

/// Puts `piece` after `out`: a block drops the spaces before it, and
/// text after a space or a line's end drops its own.
fn join(out: &mut String, piece: &str) {
    if piece.starts_with('\n') {
        out.truncate(out.trim_end_matches(' ').len());
        out.push_str(piece);
    } else if out.ends_with([' ', '\n']) {
        out.push_str(piece.trim_start_matches(' '));
    } else {
        out.push_str(piece);
    }
}

/// `text` as a block of its own, between blank lines; nothing if empty.
fn block(text: &str) -> String {
    let text = text.trim_matches([' ', '\n']);
    match text.is_empty() {
        true => String::new(),
        false => format!("\n\n{text}\n\n"),
    }
}

/// `made`, with the space `inner` had at either end.
fn spaced(inner: &str, made: String) -> String {
    let before = if inner.starts_with(' ') { " " } else { "" };
    let after = if inner.ends_with(' ') && !made.is_empty() {
        " "
    } else {
        ""
    };
    format!("{before}{made}{after}")
}

/// `text` with its lines' ends trimmed and at most one blank line
/// between blocks.
pub(super) fn compact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines() {
        let line = line.trim_end();
        blank = if line.is_empty() { blank + 1 } else { 0 };
        if blank < 2 {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// `text`'s runs of white space, a line's end among them, made one space.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        let white = c.is_ascii_whitespace() || c == '\u{a0}';
        if !(white && space) {
            out.push(if white { ' ' } else { c });
        }
        space = white;
    }
    out
}

/// `text` on one line: its lines trimmed and joined by a space.
fn squash(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines.join(" ")
}

/// Whether `line` opens or closes a fenced code block: three backticks
/// or more.
fn is_fence(line: &str) -> bool {
    line.trim_start().chars().take_while(|c| *c == '`').count() >= 3
}

/// `text` without its blank lines, but in a fenced code block: a list
/// item stays one item.
fn tighten(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.lines() {
        if is_fence(line) {
            fenced = !fenced;
        }
        if fenced || !line.trim().is_empty() {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.truncate(out.trim_end().len());
    out
}

/// `code` as an inline code span, fenced past any run of backticks in it.
fn code_span(code: &str) -> String {
    if code.is_empty() {
        return String::new();
    }
    let fence = '`'.to_string().repeat(longest_run(code, '`') + 1);
    let pad = if code.starts_with('`') || code.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{code}{pad}{fence}")
}

/// The longest run of `c` in `text`.
fn longest_run(text: &str, c: char) -> usize {
    let (mut longest, mut run) = (0, 0);
    for d in text.chars() {
        run = if d == c { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(body: &str) -> String {
        markdown(
            &format!("<html><body>{body}</body></html>"),
            &Url::parse("https://example.com/docs/page.html").unwrap(),
        )
    }

    #[test]
    fn headings_and_paragraphs_are_blocks() {
        assert_eq!(
            md("<h1>Title</h1>\n<p>One\n  line.</p><p>Two</p><h3> Small <em>part</em> </h3>text"),
            "# Title\n\nOne line.\n\nTwo\n\n### Small *part*\n\ntext"
        );
        assert_eq!(
            md("<div><div>a</div>b<br>c</div><hr><p></p>"),
            "a\n\nb\nc\n\n---"
        );
    }

    #[test]
    fn emphasis_keeps_its_spaces_outside() {
        assert_eq!(
            md("<p>a<b> bold </b>b <i>it</i>, <del>gone</del><strong></strong>.</p>"),
            "a **bold** b *it*, ~~gone~~."
        );
    }

    #[test]
    fn links_are_whole() {
        assert_eq!(
            md(
                "<p>See <a href=\"../a b\">here</a>, <a href=\"/x(1)\">x</a>, \
                <a href=\"mailto:me@example.com\">mail</a>, <a name=\"top\">top</a>, \
                <a href=\"ftp://example.com/\">ftp</a> and <a href=\"/icon\"><img src=\"i.png\"></a>.</p>"
            ),
            "See [here](https://example.com/a%20b), [x](<https://example.com/x(1)>), \
             [mail](mailto:me@example.com), top, ftp and ."
        );
    }

    #[test]
    fn lists_nest_and_count() {
        let html = "<ul><li>one</li><li>two<ul><li>two a</li><li><p>two b</p><p>more</p></li></ul></li></ul>\
            <ol start=\"9\"><li>nine</li><li>ten<ol><li>inner</li></ol></li></ol>";
        assert_eq!(
            md(html),
            "- one\n- two\n  - two a\n  - two b\n    more\n\n9. nine\n10. ten\n    1. inner"
        );
    }

    #[test]
    fn code_is_kept_as_written() {
        let html = "<p>Run <code>cargo  test</code> or <code>a`b</code>.</p>\
            <pre><code class=\"hljs language-rust\">fn main() {\n    println!(\"```\");\n}\n</code></pre>\
            <pre>plain<br>two</pre>";
        assert_eq!(
            md(html),
            "Run `cargo test` or ``a`b``.\n\n````rust\nfn main() {\n    println!(\"```\");\n}\n````\n\n```\nplain\ntwo\n```"
        );
    }

    #[test]
    fn quotes_and_tables() {
        let html = "<blockquote><p>said</p><p>twice</p><blockquote>inner</blockquote></blockquote>\
            <table><caption>Prices</caption><thead><tr><th>Item</th><th>Cost</th></tr></thead>\
            <tbody><tr><td>a|b</td><td><b>1</b></td></tr><tr><td>c</td></tr></tbody></table>";
        assert_eq!(
            md(html),
            "> said\n>\n> twice\n>\n> > inner\n\nPrices\n\n| Item | Cost |\n| --- | --- |\n| a\\|b | **1** |\n| c |  |"
        );
    }
}
