//! Words: text in a block is a [`Sentence`], a [`Phrase`] or [`Prose`],
//! never a bare `String`, so its casing and punctuation are checked once,
//! where it's made. Each holds spans, so a command or a name inside one is
//! still typed and the formatter can style it.
//!
//! The checks are of the literal parts a command writes, and run in a
//! debug build (every test): a sentence that breaks one panics there, and
//! prints as it is in a release, so a missed case is never an outage.

use std::fmt;

use serde::Serialize;

use crate::Value;

/// A piece of a sentence: words a command wrote, or a typed value.
#[derive(Clone, Debug, PartialEq)]
pub enum Span {
    Text(String),
    Value(Value),
}

/// A whole sentence: a capital first, unless it opens with a value, and
/// `.`, `?` or `!` last.
#[derive(Clone, Debug, PartialEq)]
pub struct Sentence(pub(crate) Vec<Span>);

/// A fragment: a label, a why, what a step does. One line, no end mark.
#[derive(Clone, Debug, PartialEq)]
pub struct Phrase(pub(crate) Vec<Span>);

/// One paragraph: one or more sentences, checked as a [`Sentence`] is.
#[derive(Clone, Debug, PartialEq)]
pub struct Prose(pub(crate) Vec<Span>);

/// `template`'s `{}`s filled with `values`, in order.
#[track_caller]
pub(crate) fn fill(template: &str, values: Vec<Value>) -> Vec<Span> {
    let parts: Vec<&str> = template.split("{}").collect();
    if cfg!(debug_assertions) {
        assert_eq!(
            parts.len() - 1,
            values.len(),
            "{template:?} has {} {{}} for {} values",
            parts.len() - 1,
            values.len()
        );
    }
    let mut spans = Vec::new();
    let mut values = values.into_iter();
    for (i, part) in parts.iter().enumerate() {
        if !part.is_empty() {
            check_literal(part);
            spans.push(Span::Text(part.to_string()));
        }
        if let (true, Some(value)) = (i + 1 < parts.len(), values.next()) {
            spans.push(Span::Value(value));
        }
    }
    spans
}

/// What no literal holds: Markdown, escapes, a second line, an ellipsis
/// (progress gets its own), or a label the formatter writes itself.
#[track_caller]
pub(crate) fn check_literal(text: &str) {
    if !cfg!(debug_assertions) {
        return;
    }
    for (bad, why) in [
        ("`", "a backtick: a command is a Command, shown in bold"),
        ("\x1b", "an escape: color is the formatter's"),
        ("\n", "a second line: make another block"),
        ("...", "an ellipsis: progress adds its own"),
        ("\u{2026}", "an ellipsis: progress adds its own"),
        ("Error:", "a label: a report's kind says it"),
        ("Warning:", "a label: a report's kind says it"),
        ("Note:", "a label: write the note as a sentence"),
    ] {
        assert!(!text.contains(bad), "{text:?} holds {why}");
    }
}

#[track_caller]
fn check_sentence(spans: &[Span], what: &str) {
    if !cfg!(debug_assertions) {
        return;
    }
    if let Some(Span::Text(first)) = spans.first() {
        let c = first.chars().next().unwrap_or(' ');
        assert!(
            c.is_uppercase() || c.is_ascii_digit(),
            "{what} {first:?} opens without a capital"
        );
    }
    match spans.last() {
        Some(Span::Text(last)) => assert!(
            last.ends_with(['.', '?', '!']),
            "{what} {last:?} ends without . ? or !"
        ),
        _ => panic!("{what} ends on a value, without . ? or !"),
    }
}

#[track_caller]
fn check_phrase(spans: &[Span]) {
    if !cfg!(debug_assertions) {
        return;
    }
    if let Some(Span::Text(last)) = spans.last() {
        assert!(
            !last.trim_end().ends_with(['.', ':', ';']),
            "the phrase {last:?} ends with an end mark"
        );
    }
}

impl Sentence {
    /// `template`'s `{}`s filled with `values`: what [`sentence!`] makes.
    ///
    /// [`sentence!`]: crate::sentence
    #[track_caller]
    pub fn fill(template: &str, values: Vec<Value>) -> Self {
        let spans = fill(template, values);
        check_sentence(&spans, "the sentence");
        Sentence(spans)
    }

    /// A sentence from another program, or from an error made before
    /// these types: one line, cleaned and unchecked, an end mark added
    /// where it has none. Its case is kept: it may open with a name.
    pub fn adopt(text: &str) -> Self {
        let mut out = crate::clean(text).replace('`', "").trim().to_string();
        if !out.ends_with(['.', '?', '!', ':']) {
            out.push('.');
        }
        Sentence(vec![Span::Text(out)])
    }

    pub fn spans(&self) -> &[Span] {
        &self.0
    }
}

impl Phrase {
    #[track_caller]
    pub fn fill(template: &str, values: Vec<Value>) -> Self {
        let spans = fill(template, values);
        check_phrase(&spans);
        Phrase(spans)
    }

    pub fn spans(&self) -> &[Span] {
        &self.0
    }
}

impl Prose {
    #[track_caller]
    pub fn fill(template: &str, values: Vec<Value>) -> Self {
        let spans = fill(template, values);
        check_sentence(&spans, "the prose");
        Prose(spans)
    }

    pub fn spans(&self) -> &[Span] {
        &self.0
    }
}

impl From<Sentence> for Prose {
    fn from(s: Sentence) -> Self {
        Prose(s.0)
    }
}

impl From<&str> for Phrase {
    #[track_caller]
    fn from(text: &str) -> Self {
        Phrase::fill(text, Vec::new())
    }
}

impl From<&str> for Sentence {
    #[track_caller]
    fn from(text: &str) -> Self {
        Sentence::fill(text, Vec::new())
    }
}

impl From<&str> for Prose {
    #[track_caller]
    fn from(text: &str) -> Self {
        Prose::fill(text, Vec::new())
    }
}

/// The words as plain text, every value as it shows in a pipe.
pub(crate) fn plain(spans: &[Span]) -> String {
    let ctx = crate::render::Ctx::plain();
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) => t.clone(),
            Span::Value(v) => crate::render::value(v, &ctx),
        })
        .collect()
}

macro_rules! words_as_text {
    ($($t:ty),*) => {$(
        impl fmt::Display for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&plain(&self.0))
            }
        }
        impl Serialize for $t {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&plain(&self.0))
            }
        }
    )*};
}
words_as_text!(Sentence, Phrase, Prose);

/// A [`Sentence`]: `sentence!("Couldn't start {}.", agent)`.
#[macro_export]
macro_rules! sentence {
    ($t:literal $(, $v:expr)* $(,)?) => {
        $crate::Sentence::fill($t, vec![$($crate::Value::from($v)),*])
    };
}

/// A [`Phrase`]: `phrase!("text it from your phone")`.
#[macro_export]
macro_rules! phrase {
    ($t:literal $(, $v:expr)* $(,)?) => {
        $crate::Phrase::fill($t, vec![$($crate::Value::from($v)),*])
    };
}

/// [`Prose`]: `prose!("Ollama didn't answer at {}.", address)`.
#[macro_export]
macro_rules! prose {
    ($t:literal $(, $v:expr)* $(,)?) => {
        $crate::Prose::fill($t, vec![$($crate::Value::from($v)),*])
    };
}

#[cfg(test)]
mod tests {
    use crate::{Name, Value};

    #[test]
    fn a_sentence_fills_its_values_in_order() {
        let s = sentence!("Started {} on {}.", Name::new("agent", "my-otter"), "aws");
        assert_eq!(s.to_string(), "Started my-otter on aws.");
        assert!(matches!(s.spans()[1], super::Span::Value(Value::Name(_))));
    }

    #[test]
    fn a_sentence_may_open_with_a_value() {
        let s = sentence!("{} is running.", Name::new("agent", "my-otter"));
        assert_eq!(s.to_string(), "my-otter is running.");
    }

    #[test]
    #[should_panic(expected = "without a capital")]
    fn a_sentence_opens_with_a_capital() {
        sentence!("started it.");
    }

    #[test]
    #[should_panic(expected = "ends without")]
    fn a_sentence_ends_with_a_mark() {
        sentence!("Started it");
    }

    #[test]
    #[should_panic(expected = "backtick")]
    fn no_markdown() {
        sentence!("Run `lnk up`.");
    }

    #[test]
    #[should_panic(expected = "end mark")]
    fn a_phrase_has_no_end_mark() {
        phrase!("text it from your phone.");
    }

    #[test]
    #[should_panic(expected = "has 1 {} for 0 values")]
    fn every_hole_has_a_value() {
        sentence!("Started {}.");
    }

    #[test]
    fn an_old_error_is_adopted_as_a_sentence() {
        let s = super::Sentence::adopt("no box named \"qa\"; `lnk box list` lists them");
        assert_eq!(
            s.to_string(),
            "no box named \"qa\"; lnk box list lists them."
        );
    }
}
