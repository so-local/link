//! An answer as Signal shows it. Signal takes no markup: a message is
//! plain text, and the ranges of it shown bold, italic, struck through
//! or monospace go beside it, as `signal-cli send`'s `textStyle`
//! (`<start>:<length>:<STYLE>`, counted in UTF-16 code units). A heading
//! is bold, code and tables monospace, and a link's address follows its
//! text ([`rich::styled`]).

use link_connection_contract::rich::{self, Block, Channel, Dialect, Style, Styled};
use serde::Deserialize;
use serde_json::Value;

/// What `formatting.toml` says of Signal's messages.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Formatting {
    channel: Channel,
}

static FORMATTING: std::sync::LazyLock<Formatting> = std::sync::LazyLock::new(|| {
    link_spec::parse(include_str!("formatting.toml")).expect("formatting.toml reads")
});

/// Signal's text and its styles.
pub(crate) struct Signal;

impl Dialect for Signal {
    type Out = Styled;
    fn render(&self, blocks: &[Block]) -> Styled {
        rich::styled(blocks)
    }
    fn text<'a>(&self, out: &'a Styled) -> &'a str {
        &out.text
    }
    fn channel(&self) -> &Channel {
        &FORMATTING.channel
    }
}

/// `answer` as Signal's messages, in order.
pub(crate) fn pieces(answer: &str) -> Vec<Styled> {
    rich::pieces(answer, &Signal)
        .into_iter()
        .map(|piece| piece.out)
        .collect()
}

/// `params`, a `send`'s, with `message`'s text and its styles.
pub(crate) fn with(mut params: Value, message: &Styled) -> Value {
    params["message"] = message.text.clone().into();
    let styles: Vec<Value> = message
        .utf16()
        .into_iter()
        .map(|(start, length, style)| {
            let style = match style {
                Style::Bold => "BOLD",
                Style::Italic => "ITALIC",
                Style::Strike => "STRIKETHROUGH",
                Style::Monospace => "MONOSPACE",
            };
            format!("{start}:{length}:{style}").into()
        })
        .collect();
    if !styles.is_empty() {
        params["textStyle"] = styles.into();
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sent(markdown: &str) -> Value {
        let pieces = pieces(markdown);
        assert_eq!(pieces.len(), 1, "{pieces:#?}");
        with(json!({"recipient": ["+15551234567"]}), &pieces[0])
    }

    #[test]
    fn styles_go_beside_the_text() {
        assert_eq!(
            sent("**Bold** and *it* and ~~gone~~ and `code`"),
            json!({
                "recipient": ["+15551234567"],
                "message": "Bold and it and gone and code",
                "textStyle": ["0:4:BOLD", "9:2:ITALIC", "16:4:STRIKETHROUGH", "25:4:MONOSPACE"],
            })
        );
        assert_eq!(sent("plain")["textStyle"], Value::Null);
    }

    /// Signal counts a style's start and length in UTF-16 code units: an
    /// emoji outside the Basic Multilingual Plane is two.
    #[test]
    fn styles_count_utf16_units() {
        assert_eq!(
            sent("🎉🎉 **yes** 👍🏽 _ok_"),
            json!({
                "recipient": ["+15551234567"],
                "message": "🎉🎉 yes 👍🏽 ok",
                "textStyle": ["5:3:BOLD", "14:2:ITALIC"],
            })
        );
    }

    #[test]
    fn headings_code_and_tables() {
        let sent = sent("# Plan\n\n```sh\nls\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |");
        assert_eq!(sent["message"], "Plan\n\nls\n\na | b\n--+--\n1 | 2");
        assert_eq!(
            sent["textStyle"],
            json!(["0:4:BOLD", "6:2:MONOSPACE", "10:17:MONOSPACE"])
        );
    }
}
