//! An answer as Telegram shows it: its Markdown rendered in the HTML
//! Telegram's Bot API takes (`parse_mode` `HTML`): `<b>`, `<i>`, `<s>`,
//! `<code>`, `<pre>`, `<a href>` and `<blockquote>`, a heading bold and a
//! table aligned in `<pre>`. Everything the agent wrote is escaped, so
//! no tag of its own reaches Telegram.

use link_connection_contract::rich::{self, Block, Channel, Dialect, Mark, Markup, Quoted};
use serde::Deserialize;

/// What `formatting.toml` says of Telegram's messages.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Formatting {
    pub(crate) channel: Channel,
    pub(crate) parse_mode: String,
    pub(crate) unparsed: String,
}

pub(crate) static FORMATTING: std::sync::LazyLock<Formatting> = std::sync::LazyLock::new(|| {
    link_spec::parse(include_str!("formatting.toml")).expect("formatting.toml reads")
});

/// Telegram's HTML.
pub(crate) struct Html;

impl Dialect for Html {
    type Out = String;
    fn render(&self, blocks: &[Block]) -> String {
        rich::markup(self, blocks)
    }
    fn text<'a>(&self, out: &'a String) -> &'a str {
        out
    }
    fn channel(&self) -> &Channel {
        &FORMATTING.channel
    }
}

/// One of an answer's messages: its blocks, and their HTML.
pub(crate) type Piece = rich::Piece<String>;

/// `answer` as Telegram's messages, in order.
pub(crate) fn pieces(answer: &str) -> Vec<Piece> {
    rich::pieces(answer, &Html)
}

/// A piece as plain text, for Telegram to take when it refused its HTML.
pub(crate) fn plain(piece: &Piece) -> String {
    rich::styled(&piece.blocks).text
}

/// `text` escaped for Telegram's HTML, in text or a quoted attribute.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

impl Markup for Html {
    fn text(&self, text: &str) -> String {
        escape(text)
    }
    fn mark(&self, mark: &Mark) -> (&str, &str) {
        match mark {
            Mark::Bold => ("<b>", "</b>"),
            Mark::Italic => ("<i>", "</i>"),
            _ => ("<s>", "</s>"),
        }
    }
    fn code(&self, text: &str) -> String {
        format!("<code>{}</code>", escape(text))
    }
    fn link(&self, url: &str, shown: &str, _plain: &str) -> String {
        format!("<a href=\"{}\">{shown}</a>", escape(url))
    }
    fn heading(&self, _level: u8, shown: &str, _plain: &str) -> String {
        format!("<b>{shown}</b>")
    }
    fn code_block(&self, language: Option<&str>, text: &str) -> String {
        // A language of letters, digits and `+-#._` alone: never markup.
        let language = language.filter(|l| {
            l.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-#._".contains(c))
        });
        match language {
            Some(language) => format!(
                "<pre><code class=\"language-{language}\">{}</code></pre>",
                escape(text)
            ),
            None => format!("<pre>{}</pre>", escape(text)),
        }
    }
    fn quote(&self) -> Quoted {
        Quoted::Wrapped("<blockquote>".into(), "</blockquote>".into())
    }
    fn bullet(&self, number: Option<u64>) -> String {
        number.map_or("• ".into(), |n| format!("{n}. "))
    }
    fn rule(&self) -> String {
        "———".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(markdown: &str) -> String {
        let pieces = pieces(markdown);
        assert_eq!(pieces.len(), 1, "{pieces:#?}");
        pieces[0].out.clone()
    }

    #[test]
    fn marks_are_telegrams_tags() {
        assert_eq!(
            html("# Plan\n\n**bold _both_** ~~gone~~ `a<b`"),
            "<b>Plan</b>\n\n<b>bold <i>both</i></b> <s>gone</s> <code>a&lt;b</code>"
        );
    }

    #[test]
    fn code_blocks_keep_their_language() {
        assert_eq!(
            html("```rust\nlet a = b < c && d;\n```"),
            "<pre><code class=\"language-rust\">let a = b &lt; c &amp;&amp; d;</code></pre>"
        );
        // A language that could be markup isn't named.
        assert_eq!(html("```x\"><b>\nhi\n```"), "<pre>hi</pre>");
    }

    #[test]
    fn the_agents_own_html_is_shown_never_sent() {
        assert_eq!(
            html("<script>alert(1)</script> <b>x</b> & y"),
            "&lt;script&gt;alert(1)&lt;/script&gt; &lt;b&gt;x&lt;/b&gt; &amp; y"
        );
        assert_eq!(
            html("[x](https://a.io/?q=\"><script>)"),
            "<a href=\"https://a.io/?q=%22%3E%3Cscript%3E\">x</a>"
        );
        assert_eq!(html("[x](javascript:alert(1))"), "x (javascript:alert(1))");
    }

    #[test]
    fn quotes_lists_and_tables() {
        assert_eq!(
            html("> said\n\n- a\n- b\n\n| k | v |\n|---|---|\n| x | 1 |"),
            "<blockquote>said</blockquote>\n\n• a\n• b\n\n<pre>k | v\n--+--\nx | 1</pre>"
        );
    }

    /// An answer still being written renders whole: its open fence
    /// closed, its open stars text.
    #[test]
    fn a_partial_answer_is_still_valid_html() {
        assert_eq!(
            html("**half and\n\n```py\nprint(1"),
            "**half and\n\n<pre><code class=\"language-py\">print(1</code></pre>"
        );
    }

    #[test]
    fn a_long_answer_is_cut_within_telegrams_limit() {
        let code = "x = 1 # 😀\n".repeat(600);
        let pieces = pieces(&format!("```py\n{code}```"));
        assert!(pieces.len() > 1);
        for piece in &pieces {
            assert!(piece.out.encode_utf16().count() <= 4096);
            assert!(piece.out.starts_with("<pre><code"));
            assert!(piece.out.ends_with("</code></pre>"));
        }
        assert_eq!(plain(&pieces[0]).lines().next(), Some("x = 1 # 😀"));
    }
}
