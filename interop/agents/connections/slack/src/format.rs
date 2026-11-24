//! An answer as Slack shows it: its Markdown rendered in Slack's own
//! markup, mrkdwn: `*bold*`, `_italic_`, `~strike~`, `` `code` ``, code
//! blocks between three backticks and links as `<url|text>`; a heading
//! bold and a table aligned in a code block. Everything the agent wrote
//! has `&`, `<` and `>` escaped, so an answer can't mention anyone
//! (`<!channel>`, `<@U0EVE>`) or hide a link: the only `<…>` in it are
//! the links Link writes from the Markdown's, whose URL is one
//! [`rich::web_url`] allows.

use link_connection_contract::rich::{self, Block, Channel, Dialect, Mark, Markup, Quoted};
use serde::Deserialize;

/// What `formatting.toml` says of Slack's messages.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Formatting {
    code: String,
    fence: String,
    channel: Channel,
}

static FORMATTING: std::sync::LazyLock<Formatting> = std::sync::LazyLock::new(|| {
    link_spec::parse(include_str!("formatting.toml")).expect("formatting.toml reads")
});

/// Slack's mrkdwn.
pub(crate) struct Mrkdwn;

impl Dialect for Mrkdwn {
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

/// `answer` as Slack's messages, in order, each mrkdwn.
pub(crate) fn pieces(answer: &str) -> Vec<String> {
    rich::pieces(answer, &Mrkdwn)
        .into_iter()
        .map(|piece| piece.out)
        .collect()
}

/// `text` as Slack's `text` takes it: `&`, `<` and `>` escaped, so an
/// answer can't mention anyone (`<!channel>`) or hide a link.
pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl Markup for Mrkdwn {
    fn text(&self, text: &str) -> String {
        escape(text)
    }
    fn mark(&self, mark: &Mark) -> (&str, &str) {
        match mark {
            Mark::Bold => ("*", "*"),
            Mark::Italic => ("_", "_"),
            _ => ("~", "~"),
        }
    }
    fn code(&self, text: &str) -> String {
        // Slack's inline code can't hold its own mark: such code is text.
        let code = &FORMATTING.code;
        match text.contains(code.as_str()) {
            true => escape(text),
            false => format!("{code}{}{code}", escape(text)),
        }
    }
    fn link(&self, url: &str, _shown: &str, plain: &str) -> String {
        // Slack links plain text alone; `|` would end the URL early.
        let text = escape(plain).replace('|', "¦");
        match text.trim().is_empty() || plain == url {
            true => format!("<{}>", escape(url)),
            false => format!("<{}|{text}>", escape(url)),
        }
    }
    fn heading(&self, _level: u8, _shown: &str, plain: &str) -> String {
        format!("*{}*", escape(plain))
    }
    fn code_block(&self, _language: Option<&str>, text: &str) -> String {
        let fence = &FORMATTING.fence;
        let text = rich::unfenced(&escape(text), fence);
        format!("{fence}\n{text}\n{fence}")
    }
    fn quote(&self) -> Quoted {
        Quoted::Prefixed("> ".into())
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

    fn mrkdwn(markdown: &str) -> String {
        let pieces = pieces(markdown);
        assert_eq!(pieces.len(), 1, "{pieces:#?}");
        pieces[0].clone()
    }

    #[test]
    fn marks_are_slacks() {
        assert_eq!(
            mrkdwn("## Plan\n\n**bold _both_** ~~gone~~ `x`"),
            "*Plan*\n\n*bold _both_* ~gone~ `x`"
        );
        assert_eq!(
            mrkdwn("```rust\nfn a() {}\n```\n\n- one\n- two\n\n> said"),
            "```\nfn a() {}\n```\n\n• one\n• two\n\n> said"
        );
    }

    #[test]
    fn links_are_slacks_and_only_web_ones() {
        assert_eq!(
            mrkdwn("see [the docs](https://d.io/a?b=1&c=2) or <https://e.io>"),
            "see <https://d.io/a?b=1&amp;c=2|the docs> or <https://e.io>"
        );
        assert_eq!(mrkdwn("[x](ftp://a)"), "x (ftp://a)");
    }

    /// Nothing the agent writes mentions anyone or hides a link: not in
    /// text, code, a link's text or its URL.
    #[test]
    fn answers_mention_no_one() {
        for (markdown, slack) in [
            (
                "<!channel> & <@U0EVE>",
                "&lt;!channel&gt; &amp; &lt;@U0EVE&gt;",
            ),
            ("[x](<!channel>)", "x (!channel)"),
            ("[x](<@U0EVE>)", "x (@U0EVE)"),
            ("[<!here>](https://a.io)", "<https://a.io|&lt;!here&gt;>"),
            ("[a|<@U1>](https://a.io)", "<https://a.io|a¦&lt;@U1&gt;>"),
            (
                "[x](https://a.io/|<!channel>)",
                "<https://a.io/%7C%3C!channel%3E|x>",
            ),
            ("`<!channel>`", "`&lt;!channel&gt;`"),
            ("```\n<!everyone>\n```", "```\n&lt;!everyone&gt;\n```"),
        ] {
            assert_eq!(mrkdwn(markdown), slack, "{markdown}");
        }
    }

    #[test]
    fn a_table_is_a_code_block() {
        assert_eq!(
            mrkdwn("| a | b |\n|---|---|\n| 1 | <2> |"),
            "```\na | b\n--+----\n1 | &lt;2&gt;\n```"
        );
    }
}
