//! An answer as Discord shows it: Discord reads Markdown itself, so the
//! answer is written back as the Markdown Discord knows (`**bold**`,
//! `*italic*`, `~~strike~~`, code, `[text](url)` links, headings, lists
//! and quotes), with a table aligned in a code block, which Discord
//! doesn't draw. The agent's own text has Discord's markup characters
//! escaped with `\`, so it shows as written.

use link_connection_contract::rich::{self, Block, Channel, Dialect, Mark, Markup, Quoted};
use serde::Deserialize;

/// What `formatting.toml` says of Discord's messages.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Formatting {
    escaped: String,
    line_start: String,
    code: String,
    fence: String,
    channel: Channel,
}

static FORMATTING: std::sync::LazyLock<Formatting> = std::sync::LazyLock::new(|| {
    link_spec::parse(include_str!("formatting.toml")).expect("formatting.toml reads")
});

/// Discord's Markdown.
pub(crate) struct Discord;

impl Dialect for Discord {
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

/// `answer` as Discord's messages, in order.
pub(crate) fn pieces(answer: &str) -> Vec<String> {
    rich::pieces(answer, &Discord)
        .into_iter()
        .map(|piece| piece.out)
        .collect()
}

/// `text` with Discord's markup escaped, but in a web address, which
/// Discord links as written.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut line_start = true;
    for word in text.split_inclusive(char::is_whitespace) {
        let address = word.contains("://");
        for (j, c) in word.chars().enumerate() {
            let starts = line_start && j == 0;
            let markup = |set: &str| set.contains(c);
            if (!address && markup(&FORMATTING.escaped))
                || (starts && markup(&FORMATTING.line_start))
            {
                out.push('\\');
            }
            out.push(c);
        }
        line_start = word.ends_with('\n');
    }
    out
}

impl Markup for Discord {
    fn text(&self, text: &str) -> String {
        escape(text)
    }
    fn mark(&self, mark: &Mark) -> (&str, &str) {
        match mark {
            Mark::Bold => ("**", "**"),
            Mark::Italic => ("*", "*"),
            _ => ("~~", "~~"),
        }
    }
    fn code(&self, text: &str) -> String {
        // Code holding its own mark goes between two of them, spaced.
        let code = &FORMATTING.code;
        match text.contains(code.as_str()) {
            true => format!("{code}{code} {text} {code}{code}"),
            false => format!("{code}{text}{code}"),
        }
    }
    fn link(&self, url: &str, shown: &str, plain: &str) -> String {
        match plain == url {
            true => url.to_string(),
            false => format!("[{shown}]({url})"),
        }
    }
    fn heading(&self, level: u8, shown: &str, _plain: &str) -> String {
        match level {
            1..=3 => format!("{} {shown}", "#".repeat(level.into())),
            _ => format!("**{shown}**"),
        }
    }
    fn code_block(&self, language: Option<&str>, text: &str) -> String {
        let language = language
            .filter(|l| {
                l.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-#._".contains(c))
            })
            .unwrap_or_default();
        let fence = &FORMATTING.fence;
        let text = rich::unfenced(text, fence);
        format!("{fence}{language}\n{text}\n{fence}")
    }
    fn quote(&self) -> Quoted {
        Quoted::Prefixed("> ".into())
    }
    fn bullet(&self, number: Option<u64>) -> String {
        number.map_or("- ".into(), |n| format!("{n}. "))
    }
    fn rule(&self) -> String {
        "———".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discord(markdown: &str) -> String {
        let pieces = pieces(markdown);
        assert_eq!(pieces.len(), 1, "{pieces:#?}");
        pieces[0].clone()
    }

    #[test]
    fn markdown_passes_through() {
        let markdown = "# Plan\n\n**bold *both*** ~~gone~~ `x`\n\n```rust\nfn a() {}\n```\n\n\
                        - one\n- two\n\n> said\n\n[docs](https://d.io)";
        assert_eq!(discord(markdown), markdown);
    }

    #[test]
    fn a_table_is_a_code_block() {
        assert_eq!(
            discord("| a | b |\n|---|---|\n| 1 | 2 |"),
            "```\na | b\n--+--\n1 | 2\n```"
        );
    }

    #[test]
    fn the_agents_text_shows_as_written() {
        assert_eq!(discord("2 \\* 3 = 6"), "2 \\* 3 = 6");
        assert_eq!(discord("``a`b`` and *"), "`` a`b `` and \\*");
        assert_eq!(
            discord("see https://x.io/a_b_c for <@123>"),
            "see https://x.io/a_b_c for \\<@123>"
        );
    }

    #[test]
    fn a_long_answer_is_cut_within_discords_limit() {
        let code = "let x = 1;\n".repeat(400);
        let pieces = pieces(&format!("```rust\n{code}```"));
        assert!(pieces.len() > 1);
        for piece in &pieces {
            assert!(piece.chars().count() <= 2000);
            assert!(piece.starts_with("```rust\n") && piece.ends_with("\n```"));
        }
    }
}
