//! A command, as words, so `lnk agent conect` can't be written without a
//! test finding it and Next's graph can read every one.

use serde::Serialize;

use crate::Value;

/// A command to type: its words, each fixed, a placeholder (`<model>`)
/// or a value filled in when it's made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command(pub(crate) Vec<Word>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Word {
    Fixed(String),
    /// `<model>`: the person fills it in.
    Placeholder(String),
    /// A value filled in, shell-quoted when shown if it needs it.
    Value(String),
}

impl Command {
    /// `template`'s words, split on spaces: `<x>` a placeholder, `{}` the
    /// next of `values`. What [`cmd!`] makes.
    ///
    /// [`cmd!`]: crate::cmd
    #[track_caller]
    pub fn fill(template: &str, values: Vec<Value>) -> Self {
        let mut values = values.into_iter();
        let words = template
            .split_whitespace()
            .map(|w| {
                if w == "{}" {
                    let v = values.next().expect("a {} without its value");
                    Word::Value(crate::words::plain(&[crate::Span::Value(v)]))
                } else if w.starts_with('<') && w.ends_with('>') && w.len() > 2 {
                    Word::Placeholder(w[1..w.len() - 1].to_string())
                } else {
                    crate::words::check_literal(w);
                    Word::Fixed(w.to_string())
                }
            })
            .collect::<Vec<_>>();
        if cfg!(debug_assertions) {
            assert!(values.next().is_none(), "{template:?} has values left over");
            assert!(
                matches!(words.first(), Some(Word::Fixed(_))),
                "{template:?} opens without a program"
            );
        }
        Command(words)
    }

    /// The command a person typed, from its arguments: what a retry is.
    pub fn typed(args: impl IntoIterator<Item = String>) -> Self {
        Command(
            args.into_iter()
                .map(|a| Word::Value(crate::clean(&a)))
                .collect(),
        )
    }

    pub fn words(&self) -> &[Word] {
        &self.0
    }

    /// The command with `more` after it: `--model <model>` on a retry.
    /// Before a `--`, past which no word is a flag: `lnk sandbox run --yes --
    /// true`.
    #[track_caller]
    pub fn and(mut self, more: &str) -> Self {
        let at = self
            .0
            .iter()
            .position(|w| matches!(w, Word::Fixed(w) | Word::Value(w) if w == "--"))
            .unwrap_or(self.0.len());
        let more = Command::fill(&format!("x {more}"), Vec::new()).0;
        self.0.splice(at..at, more.into_iter().skip(1));
        self
    }

    /// The command with the value of each flag in `secret` (`--token`)
    /// shown as `<secret>`, whether it follows the flag or is joined to it
    /// by `=`: nothing else of what was typed leaves a record or a retry.
    pub fn hiding(&self, secret: &[&str]) -> Command {
        let mut words = Vec::with_capacity(self.0.len());
        let mut hidden = false;
        for word in &self.0 {
            let (Word::Fixed(text) | Word::Value(text)) = word else {
                words.push(word.clone());
                continue;
            };
            if std::mem::take(&mut hidden) {
                words.push(Word::Placeholder("secret".into()));
            } else if let Some(flag) = secret.iter().find(|flag| text == *flag) {
                hidden = true;
                words.push(Word::Fixed((*flag).to_string()));
            } else if let Some(flag) = secret.iter().find(|flag| {
                text.strip_prefix(**flag)
                    .is_some_and(|r| r.starts_with('='))
            }) {
                words.push(Word::Fixed(format!("{flag}=<secret>")));
            } else {
                words.push(word.clone());
            }
        }
        Command(words)
    }

    /// Whether it's `lnk`'s, as opposed to another program's.
    pub fn is_lnk(&self) -> bool {
        matches!(self.0.first(), Some(Word::Fixed(w) | Word::Value(w)) if w == "lnk")
    }

    /// The words up to its first flag, placeholder or value: what names
    /// the command itself (`lnk agent start`), its arguments left out.
    pub fn path(&self) -> Vec<&str> {
        self.0
            .iter()
            .map_while(|w| match w {
                Word::Fixed(f) if !f.starts_with('-') => Some(f.as_str()),
                _ => None,
            })
            .collect()
    }

    /// As it shows: words joined by spaces, a value quoted if it must be.
    pub fn text(&self) -> String {
        self.0
            .iter()
            .map(|w| match w {
                Word::Fixed(f) => f.clone(),
                Word::Placeholder(p) => format!("<{p}>"),
                Word::Value(v) => quoted(v),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn quoted(v: &str) -> String {
    let safe = !v.is_empty()
        && v.chars()
            .all(|c| c.is_alphanumeric() || "-_./:=@,+~".contains(c));
    match safe {
        true => v.to_string(),
        false => format!("'{}'", v.replace('\'', r"'\''")),
    }
}

impl Serialize for Command {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let words: Vec<String> = self
            .0
            .iter()
            .map(|w| match w {
                Word::Fixed(f) | Word::Value(f) => f.clone(),
                Word::Placeholder(p) => format!("<{p}>"),
            })
            .collect();
        words.serialize(s)
    }
}

/// A [`Command`]: `cmd!("lnk agent start --model.default <model>")`,
/// `cmd!("lnk box ssh {}", name)`.
#[macro_export]
macro_rules! cmd {
    ($t:literal $(, $v:expr)* $(,)?) => {
        $crate::Command::fill($t, vec![$($crate::Value::from($v)),*])
    };
}

#[cfg(test)]
mod tests {
    use crate::{Command, Name};

    #[test]
    fn a_command_has_its_words() {
        let c = cmd!("lnk box ssh {} --port <port>", Name::new("box", "qa 1"));
        assert_eq!(c.text(), "lnk box ssh 'qa 1' --port <port>");
        assert_eq!(c.path(), ["lnk", "box", "ssh"]);
        assert!(c.is_lnk());
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"["lnk","box","ssh","qa 1","--port","<port>"]"#
        );
    }

    #[test]
    fn a_secret_flags_value_is_hidden_either_way_it_is_typed() {
        let typed = Command::typed(
            [
                "lnk",
                "agent",
                "connect",
                "--token",
                "s3cret",
                "--agent=x",
                "--token=also",
                "--json",
            ]
            .map(String::from),
        );
        let hidden = typed.hiding(&["--token"]);
        assert_eq!(
            hidden.text(),
            "lnk agent connect --token <secret> --agent=x --token=<secret> --json"
        );
        assert!(!serde_json::to_string(&hidden).unwrap().contains("s3cret"));
        assert_eq!(typed.hiding(&[]), typed);
    }

    #[test]
    fn a_retry_can_take_more() {
        let c = cmd!("lnk agent start").and("--model.default <model>");
        assert_eq!(c.text(), "lnk agent start --model.default <model>");
    }

    #[test]
    fn more_goes_before_a_double_dash() {
        let typed = Command::typed(["lnk", "sandbox", "run", "--", "true"].map(String::from));
        assert_eq!(typed.and("--yes").text(), "lnk sandbox run --yes -- true");
    }
}
