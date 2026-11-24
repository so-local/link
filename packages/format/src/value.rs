//! Values: what goes in a span, a cell or a field. Each knows how it shows
//! on a screen and in JSON, so a command passes the thing, never its text.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde::ser::SerializeMap;

use crate::{Command, State, Tone};

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// A thing Link or the person named: an agent, a box, a module.
    Name(Name),
    /// A product, as its maker writes it: Telegram, GitHub.
    Product(Product),
    /// A command: bold, never inside backticks.
    Command(Command),
    Path(Path),
    Address(Address),
    State(StateWord),
    Size(Size),
    Elapsed(Elapsed),
    Time(Time),
    Money(Money),
    Count(Count),
    Version(Version),
    /// `<secret>`: no constructor takes the value.
    Secret,
    /// Text another program made: cleaned, never styled.
    Foreign(Foreign),
    /// Anything else, plain: `aws us-east`.
    Text(String),
    /// `telegram, slack and discord`.
    List(Vec<Value>),
    /// A mark on its own, in a cell or a field: `✓` for a permission
    /// allowed, `!` for one beyond the defaults.
    Mark(crate::look::Mark),
}

/// A name and its kind, `agent` or `box`: the kind is the module's own
/// word, so `--json` says what it names and an issue can stand in for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Name {
    pub(crate) kind: &'static str,
    pub(crate) name: String,
    pub(crate) typed: bool,
}

impl Name {
    pub fn new(kind: &'static str, name: impl Into<String>) -> Self {
        Name {
            kind,
            name: crate::clean(&name.into()),
            typed: false,
        }
    }

    /// A name as the person typed it, which may not exist: shown in
    /// double quotes, so an empty or spaced one shows.
    pub fn typed(kind: &'static str, name: impl Into<String>) -> Self {
        Name {
            typed: true,
            ..Name::new(kind, name)
        }
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Product(pub(crate) String);

impl Product {
    pub fn new(name: impl Into<String>) -> Self {
        Product(crate::clean(&name.into()))
    }
}

/// A path on this machine: home as `~`, a link where the terminal takes
/// one, never colored; `--json` has it whole.
#[derive(Clone, Debug, PartialEq)]
pub struct Path(pub(crate) PathBuf);

impl Path {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Path(path.into())
    }

    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

/// An address: `203.0.113.4:22`, `https://...`. Never cut, since it's
/// copied.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Address(pub(crate) String);

impl Address {
    pub fn new(address: impl Into<String>) -> Self {
        Address(crate::clean(&address.into()))
    }

    /// `localhost:<port>`.
    pub fn local(port: u16) -> Self {
        Address(format!("localhost:{port}"))
    }
}

/// A state's word and its tone, from a module's own [`State`] enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateWord {
    pub(crate) word: &'static str,
    pub(crate) tone: Tone,
}

impl StateWord {
    pub fn of(state: &impl State) -> Self {
        StateWord {
            word: state.word(),
            tone: state.tone(),
        }
    }
}

/// A size in bytes, shown 1,000-based: `4 GB`, `512 MB`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Size(pub u64);

/// How long something took or lasts: `3m 20s`, `2h 5m`, `4d`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Elapsed(pub Duration);

/// A moment: `5 min ago` when recent, else `2026-10-23`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Time(pub SystemTime);

impl Serialize for Time {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::time::rfc3339(self.0))
    }
}

/// Money in cents, perhaps by a unit: `$0.12/h`, `$4.30`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Money {
    pub cents: u64,
    pub per: Option<&'static str>,
}

/// A count and its noun, both forms: `1 box`, `3 boxes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Count {
    pub n: u64,
    pub one: &'static str,
    pub many: &'static str,
}

impl Count {
    pub fn new(n: u64, one: &'static str, many: &'static str) -> Self {
        Count { n, one, many }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Version(pub(crate) String);

impl Version {
    pub fn new(version: impl Into<String>) -> Self {
        Version(crate::clean(&version.into()))
    }
}

/// Text another program made: cleaned when it's made, so it can't bring
/// escapes, and never styled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Foreign(pub(crate) String);

impl Foreign {
    pub fn new(text: &str) -> Self {
        Foreign(crate::clean(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! value_from {
    ($($t:ty => $v:ident),* $(,)?) => {$(
        impl From<$t> for Value {
            fn from(x: $t) -> Self { Value::$v(x) }
        }
    )*};
}
value_from!(
    Name => Name, Product => Product, Command => Command, Path => Path,
    Address => Address, StateWord => State, Size => Size, Elapsed => Elapsed,
    Time => Time, Money => Money, Count => Count, Version => Version,
    Foreign => Foreign, Vec<Value> => List, crate::look::Mark => Mark,
);

impl From<&str> for Value {
    #[track_caller]
    fn from(text: &str) -> Self {
        crate::words::check_literal(text);
        Value::Text(text.to_string())
    }
}

impl From<String> for Value {
    fn from(text: String) -> Self {
        Value::Text(crate::clean(&text))
    }
}

impl From<&String> for Value {
    fn from(text: &String) -> Self {
        Value::Text(crate::clean(text))
    }
}

impl From<u64> for Value {
    fn from(n: u64) -> Self {
        Value::Text(crate::render::thousands(n))
    }
}

impl From<usize> for Value {
    fn from(n: usize) -> Self {
        Value::from(n as u64)
    }
}

impl From<&Value> for Value {
    fn from(v: &Value) -> Self {
        v.clone()
    }
}

impl From<&Name> for Value {
    fn from(n: &Name) -> Self {
        Value::Name(n.clone())
    }
}

impl Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let one = |key: &str, s: S, v: &dyn erased::Ser| -> Result<S::Ok, S::Error> {
            let mut m = s.serialize_map(Some(1))?;
            m.serialize_entry(key, &erased::Wrap(v))?;
            m.end()
        };
        match self {
            Value::Name(n) => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry(n.kind, &n.name)?;
                m.end()
            }
            Value::Product(p) => one("product", s, p),
            Value::Command(c) => one("command", s, c),
            Value::Path(p) => one("path", s, &p.0.display().to_string()),
            Value::Address(a) => one("address", s, a),
            Value::State(w) => {
                let mut m = s.serialize_map(Some(2))?;
                m.serialize_entry("state", w.word)?;
                m.serialize_entry("tone", &w.tone)?;
                m.end()
            }
            Value::Size(b) => one("bytes", s, &b.0),
            Value::Elapsed(e) => one("seconds", s, &e.0.as_secs_f64()),
            Value::Time(t) => one("time", s, &crate::time::rfc3339(t.0)),
            Value::Money(m) => one("money", s, m),
            Value::Count(c) => s.serialize_u64(c.n),
            Value::Version(v) => one("version", s, v),
            Value::Secret => s.serialize_str("<secret>"),
            Value::Foreign(f) => s.serialize_str(&f.0),
            Value::Text(t) => s.serialize_str(t),
            Value::List(l) => l.serialize(s),
            Value::Mark(m) => one("mark", s, m),
        }
    }
}

/// Serializing a value behind `&dyn`, for [`Value`]'s one-key maps.
mod erased {
    pub trait Ser {
        fn json(&self) -> serde_json::Value;
    }
    impl<T: serde::Serialize> Ser for T {
        fn json(&self) -> serde_json::Value {
            serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
        }
    }
    pub struct Wrap<'a>(pub &'a dyn Ser);
    impl serde::Serialize for Wrap<'_> {
        fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            self.0.json().serialize(s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(v: impl Into<Value>) -> String {
        serde_json::to_string(&v.into()).unwrap()
    }

    #[test]
    fn values_say_what_they_are_in_json() {
        assert_eq!(
            json(Name::new("agent", "my-otter")),
            r#"{"agent":"my-otter"}"#
        );
        assert_eq!(json(Size(4_000_000_000)), r#"{"bytes":4000000000}"#);
        assert_eq!(json(Count::new(3, "box", "boxes")), "3");
        assert_eq!(json(Value::Secret), r#""<secret>""#);
        assert_eq!(json(crate::look::Mark::Warning), r#"{"mark":"warning"}"#);
        assert_eq!(
            json(Money {
                cents: 12,
                per: Some("h")
            }),
            r#"{"money":{"cents":12,"per":"h"}}"#
        );
    }

    #[test]
    fn text_from_elsewhere_is_cleaned() {
        assert_eq!(Foreign::new("a\x1b[31mb\u{202E}").as_str(), "a[31mb");
        assert_eq!(Name::new("box", "qa\x07-1").as_str(), "qa-1");
    }
}
