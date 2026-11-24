//! The look: every role, symbol and spacing in one place, so tuning how
//! `lnk` looks is this file, and no command moves. And output's
//! settings, the formatter's axes: `[output]` in `~/Link/Safe/settings.toml`,
//! each also a variable of its key's name (`LNK_OUTPUT_COLOR`).

use anstyle::{AnsiColor, Style};

use crate::Tone;

/// How far a result is indented.
pub const INDENT: &str = "  ";
/// A list item's bullet, shared by terminal and plain output.
pub const BULLET: &str = "- ";
/// Prose wraps here, or at the terminal's width if it's less.
pub const WIDTH: usize = 80;
/// Next's heading.
pub const NEXT: &str = "Next";
/// At most this many suggestions show.
pub const MOST_NEXT: usize = 5;
/// A spinner's frames, and how often one turns.
pub const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub const SPIN_EVERY: std::time::Duration = std::time::Duration::from_millis(100);
/// How often a wait with no terminal to draw on says it's still going,
/// so a log or a pipe never goes quiet for minutes.
pub const STILL_EVERY: std::time::Duration = std::time::Duration::from_secs(60);
/// A bar's width, in characters, and what it's drawn with: ASCII, so
/// every font has it.
pub const BAR: usize = 16;
pub const BAR_DONE: char = '#';
pub const BAR_LEFT: char = '-';
/// Between a table's columns, a tree's cells and a field's label and
/// value.
pub const GAP: &str = "  ";
/// Between a Next's command and its why, the whys lined up after it.
pub const WHY_GAP: usize = 3;
/// The one ellipsis: the end of a progress line, still going.
pub const ELLIPSIS: &str = "...";
/// Between the parts of a report's context line.
pub const SEPARATOR: &str = " \u{b7} ";
/// A tree's branches: a child, the last child, and the line past a child
/// with more below.
pub const BRANCH: &str = "\u{251c}\u{2500}\u{2500} ";
pub const LAST_BRANCH: &str = "\u{2514}\u{2500}\u{2500} ";
pub const TRUNK: &str = "\u{2502}   ";

/// The color of a tone's word: the terminal's own 16, so its theme sets
/// the shade.
pub const fn tone(tone: Tone) -> Style {
    match tone {
        Tone::Good => AnsiColor::Green.on_default(),
        Tone::Quiet => Style::new(),
        Tone::Warn => AnsiColor::Yellow.on_default(),
        Tone::Bad => AnsiColor::Red.on_default(),
    }
}

/// Faint: what places a line rather than says it, such as a log line's
/// time and IDs.
pub const FAINT: Style = Style::new().dimmed();

/// Bold: every command, a report's headline, the one key value, headings.
pub const BOLD: Style = Style::new().bold();

/// What a report or a success opens with, or a cell holds as a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mark {
    Success,
    /// Done as far as it can go: the rest waits on the person.
    Waiting,
    Warning,
    Error,
    Refusal,
}

/// A mark in a setting of `symbols`, and its tone. `None` writes a word.
pub fn mark(mark: Mark, symbols: Symbols) -> (&'static str, Tone) {
    match (mark, symbols) {
        (Mark::Success, Symbols::Emoji) => ("\u{2705}", Tone::Good),
        (Mark::Waiting, Symbols::Emoji) => ("\u{231b}", Tone::Warn),
        // Two spaces: many terminals draw ⚠️ wider than they count it.
        (Mark::Warning, Symbols::Emoji) => ("\u{26a0}\u{fe0f} ", Tone::Warn),
        (Mark::Error, Symbols::Emoji) => ("\u{274c}", Tone::Bad),
        (Mark::Refusal, Symbols::Emoji) => ("\u{26d4}", Tone::Bad),
        (Mark::Success, Symbols::Plain) => ("\u{2713}", Tone::Good),
        (Mark::Waiting, Symbols::Plain) => ("\u{2026}", Tone::Warn),
        (Mark::Warning, Symbols::Plain) => ("!", Tone::Warn),
        (Mark::Error | Mark::Refusal, Symbols::Plain) => ("\u{2717}", Tone::Bad),
        (Mark::Success, Symbols::None) => ("", Tone::Quiet),
        (Mark::Waiting, Symbols::None) => ("waiting:", Tone::Warn),
        (Mark::Warning, Symbols::None) => ("warning:", Tone::Warn),
        (Mark::Error, Symbols::None) => ("error:", Tone::Bad),
        (Mark::Refusal, Symbols::None) => ("refused:", Tone::Bad),
    }
}

/// A mark as a value, in a cell or a field (a check, a row's state):
/// never an emoji, whose width a terminal may draw wider than it counts,
/// so columns stay lined up.
pub fn value_mark(mark: Mark, symbols: Symbols) -> (&'static str, Tone) {
    match (mark, symbols) {
        (Mark::Success, Symbols::None) => ("ok", Tone::Good),
        (Mark::Waiting, Symbols::None) => ("wait", Tone::Warn),
        (Mark::Warning, Symbols::None) => ("warn", Tone::Warn),
        (Mark::Error | Mark::Refusal, Symbols::None) => ("fail", Tone::Bad),
        (mark, _) => self::mark(mark, Symbols::Plain),
    }
}

/// `auto`, `always` or `never`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    Auto,
    Always,
    Never,
}

/// `emoji`, `plain` (✓ ! ✗) or `none`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Symbols {
    Emoji,
    Plain,
    None,
}

/// Output's settings, `[output]` in `~/Link/Safe/settings.toml`, each key
/// also a variable ([`variable`]): a variable wins over the file, and
/// the file over the default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub color: Choice,
    pub emphasis: Choice,
    pub symbols: Symbols,
    pub links: Choice,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            color: Choice::Auto,
            emphasis: Choice::Auto,
            symbols: Symbols::Emoji,
            links: Choice::Auto,
        }
    }
}

/// The variable a setting is also: `LNK_`, its table and its key, in
/// capitals (`[output] color` is `LNK_OUTPUT_COLOR`). Every setting's
/// name comes from here, so a key and its variable never drift.
pub fn variable(table: &str, key: &str) -> String {
    format!("LNK_{table}_{key}")
        .to_uppercase()
        .replace('-', "_")
}

/// The table output's settings are in.
pub const TABLE: &str = "output";

impl Settings {
    /// The defaults, under the variables.
    pub fn from_env() -> Self {
        Settings::default().under_env()
    }

    /// These, from the settings file, under the variables.
    pub fn under_env(self) -> Self {
        self.under(|name| std::env::var(name).ok())
    }

    /// The defaults, under the variables `var` gives.
    pub fn from(var: impl Fn(&str) -> Option<String>) -> Self {
        Settings::default().under(var)
    }

    /// These under the variables `var` gives: each key's, then
    /// `NO_COLOR` and `CLICOLOR_FORCE`.
    pub fn under(self, var: impl Fn(&str) -> Option<String>) -> Self {
        let mut s = self;
        let key = |key: &str| var(&variable(TABLE, key));
        let choice = |v: Option<String>| match v.as_deref().map(str::trim) {
            Some("always" | "on" | "1" | "true" | "yes") => Some(Choice::Always),
            Some("never" | "off" | "0" | "false" | "no") => Some(Choice::Never),
            Some("auto") => Some(Choice::Auto),
            _ => None,
        };
        s.color = choice(key("color")).unwrap_or(s.color);
        s.emphasis = choice(key("emphasis")).unwrap_or(s.emphasis);
        s.links = choice(key("links")).unwrap_or(s.links);
        if var("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            s.color = Choice::Never;
        } else if var("CLICOLOR_FORCE").is_some_and(|v| !v.is_empty() && v != "0") {
            s.color = Choice::Always;
        }
        s.symbols = match key("symbols").as_deref().map(str::trim) {
            Some("emoji") => Symbols::Emoji,
            Some("plain") => Symbols::Plain,
            Some("none") => Symbols::None,
            _ => s.symbols,
        };
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(pairs: &[(&str, &str)]) -> Settings {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Settings::from(move |n| pairs.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()))
    }

    #[test]
    fn a_setting_is_its_tables_variable_and_wins_over_the_file() {
        assert_eq!(variable("output", "color"), "LNK_OUTPUT_COLOR");
        let file: Settings = from_json(r#"{"symbols": "plain", "color": "never"}"#);
        assert_eq!(file.under(|_| None).symbols, Symbols::Plain);
        let over = file.under(|n| (n == "LNK_OUTPUT_SYMBOLS").then(|| "none".into()));
        assert_eq!((over.symbols, over.color), (Symbols::None, Choice::Never));
        assert!(serde_json::from_str::<Settings>(r#"{"colour": "never"}"#).is_err());
    }

    fn from_json(json: &str) -> Settings {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn settings_come_from_their_variables() {
        assert_eq!(with(&[]), Settings::default());
        assert_eq!(with(&[("LNK_OUTPUT_COLOR", "never")]).color, Choice::Never);
        assert_eq!(
            with(&[("NO_COLOR", "1"), ("LNK_OUTPUT_COLOR", "always")]).color,
            Choice::Never
        );
        assert_eq!(
            with(&[("LNK_OUTPUT_SYMBOLS", "plain")]).symbols,
            Symbols::Plain
        );
        assert_eq!(
            with(&[("LNK_OUTPUT_SYMBOLS", "none")]).symbols,
            Symbols::None
        );
    }
}
