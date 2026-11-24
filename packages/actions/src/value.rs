//! What generated types call to refuse a value its shape refuses: one
//! check per keyword, each naming where the value is and why, so a spec
//! file and its flags are refused alike.

use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{LazyLock, Mutex},
};

mod duration;
mod rate;
mod size;
pub use duration::Duration;
pub use rate::Rate;
pub use size::Size;

/// A text that is one of `words`, read as a choice's word: any other text
/// is refused, so an untagged choice reads it as its next variant.
pub fn word<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
    words: &[&str],
) -> Result<String, D::Error> {
    use serde::Deserialize;
    let text = String::deserialize(deserializer)?;
    match words.contains(&text.as_str()) {
        true => Ok(text),
        false => Err(serde::de::Error::custom(format!(
            "{text:?} is none of {}",
            words.join(", ")
        ))),
    }
}

/// `"16 MB"` as `("16", "MB")`: its leading digits and the word after,
/// the space between optional.
fn number_and_word(text: &str) -> (&str, &str) {
    let text = text.trim();
    let at = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, word) = text.split_at(at);
    (number, word.trim())
}

/// Why a value was refused, and where it is: `stages[1].timeout`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub at: String,
    pub why: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.at.is_empty() {
            f.write_str(&self.why)
        } else {
            write!(f, "{}: {}", self.at, self.why)
        }
    }
}

impl std::error::Error for Refusal {}

fn refuse(at: &str, why: String) -> Result<(), Refusal> {
    Err(Refusal {
        at: at.to_string(),
        why,
    })
}

/// Where a field is, below `at`: `env.name`, or `name` at the top.
pub fn field(at: &str, name: &str) -> String {
    if at.is_empty() {
        name.to_string()
    } else {
        format!("{at}.{name}")
    }
}

/// What a generated type checks beyond what reading it already did.
pub trait Validate {
    fn validate(&self, at: &str) -> Result<(), Refusal>;
}

/// A closed choice of words, as the generator writes every `enum`: each
/// value with the word it's written as, in declared order, so code names a
/// word only through its declaration.
pub trait Words: Copy + PartialEq + 'static {
    /// Every value, in order, each with its word.
    const WORDS: &'static [(Self, &'static str)];

    /// What each value means, in order, where the declaration says it
    /// (`x-link-values`); empty where it doesn't.
    const MEANINGS: &'static [(Self, &'static str)] = &[];

    /// Every value, in declared order.
    fn all() -> impl Iterator<Item = Self> {
        Self::WORDS.iter().map(|(value, _)| *value)
    }

    /// The word it's written as.
    fn word(self) -> &'static str {
        Self::WORDS
            .iter()
            .find(|(value, _)| *value == self)
            .map(|(_, word)| *word)
            .expect("its declaration writes every value")
    }

    /// The value written as `word`.
    fn from_word(word: &str) -> Option<Self> {
        Self::WORDS
            .iter()
            .find(|(_, written)| *written == word)
            .map(|(value, _)| *value)
    }

    /// What it means, where its declaration says (`x-link-values`).
    fn meaning(self) -> Option<&'static str> {
        Self::MEANINGS
            .iter()
            .find(|(value, _)| *value == self)
            .map(|(_, meaning)| *meaning)
    }
}

macro_rules! plain {
    ($($ty:ty),*) => {$(
        impl Validate for $ty {
            fn validate(&self, _: &str) -> Result<(), Refusal> {
                Ok(())
            }
        }
    )*};
}
plain!(
    (),
    String,
    bool,
    i64,
    f64,
    std::path::PathBuf,
    Duration,
    Rate,
    Size,
    serde_json::Value
);

impl<T: Validate + ?Sized> Validate for &T {
    fn validate(&self, at: &str) -> Result<(), Refusal> {
        (**self).validate(at)
    }
}

impl<T: Validate + ?Sized> Validate for Box<T> {
    fn validate(&self, at: &str) -> Result<(), Refusal> {
        (**self).validate(at)
    }
}

impl<T: Validate> Validate for Option<T> {
    fn validate(&self, at: &str) -> Result<(), Refusal> {
        self.as_ref().map_or(Ok(()), |value| value.validate(at))
    }
}

impl<T: Validate> Validate for Vec<T> {
    fn validate(&self, at: &str) -> Result<(), Refusal> {
        self.iter()
            .enumerate()
            .try_for_each(|(i, value)| value.validate(&format!("{at}[{}]", i + 1)))
    }
}

impl<T: Validate> Validate for BTreeMap<String, T> {
    fn validate(&self, at: &str) -> Result<(), Refusal> {
        self.iter()
            .try_for_each(|(key, value)| value.validate(&field(at, key)))
    }
}

/// `pattern`: `text` holds a match, as JSON Schema reads a pattern
/// (anchor it with `^` and `$` to match the whole).
/// A secret's text, read in place for the checks its declaration names
/// (its pattern and length), never copied or kept.
pub fn secret_text(secret: &link_secretive::Secret<String>) -> &str {
    secret.expose()
}

pub fn pattern(at: &str, text: &str, pattern: &'static str) -> Result<(), Refusal> {
    described(at, text, pattern, None)
}

/// [`pattern`], its refusal ending with what the value is (`said`), the
/// shape's description, so a person learns what to write instead.
pub fn described(
    at: &str,
    text: &str,
    pattern: &'static str,
    said: Option<&str>,
) -> Result<(), Refusal> {
    static COMPILED: LazyLock<Mutex<HashMap<&'static str, regex::Regex>>> =
        LazyLock::new(Mutex::default);
    let mut compiled = COMPILED.lock().unwrap_or_else(|e| e.into_inner());
    let regex = match compiled.get(pattern) {
        Some(regex) => regex,
        None => {
            // A built-in pattern compiled when its declaration was read.
            let Ok(regex) = regex::Regex::new(pattern) else {
                return refuse(at, format!("its pattern {pattern:?} doesn't compile"));
            };
            compiled.entry(pattern).or_insert(regex)
        }
    };
    if regex.is_match(text) {
        Ok(())
    } else {
        match said {
            Some(said) => refuse(at, format!("{text:?} doesn't match {pattern}: {said}")),
            None => refuse(at, format!("{text:?} doesn't match {pattern}")),
        }
    }
}

/// `minLength` and `maxLength`, in characters.
pub fn length(at: &str, text: &str, min: Option<u64>, max: Option<u64>) -> Result<(), Refusal> {
    let n = text.chars().count() as u64;
    match (min, max) {
        (Some(min), _) if n < min => refuse(at, format!("is shorter than {min} characters")),
        (_, Some(max)) if n > max => refuse(at, format!("is longer than {max} characters")),
        _ => Ok(()),
    }
}

/// `minItems` and `maxItems`.
pub fn items(at: &str, n: usize, min: Option<u64>, max: Option<u64>) -> Result<(), Refusal> {
    let n = n as u64;
    match (min, max) {
        (Some(min), _) if n < min => refuse(at, format!("lists fewer than {min}")),
        (_, Some(max)) if n > max => refuse(at, format!("lists more than {max}")),
        _ => Ok(()),
    }
}

/// `minimum` and `maximum`.
pub fn bounds<T: PartialOrd + fmt::Display>(
    at: &str,
    value: T,
    min: Option<T>,
    max: Option<T>,
) -> Result<(), Refusal> {
    match (min, max) {
        (Some(min), _) if value < min => refuse(at, format!("{value} is below {min}")),
        (_, Some(max)) if value > max => refuse(at, format!("{value} is above {max}")),
        _ => Ok(()),
    }
}

/// A Link ID of `kind`, `<kind>:<id>`.
pub fn id(at: &str, text: &str, kind: &str) -> Result<(), Refusal> {
    match text.split_once(':') {
        Some((k, id)) if k == kind && link_id::is_id(id) => Ok(()),
        _ => refuse(at, format!("{text:?} isn't a {kind} ID, {kind}:<id>")),
    }
}

/// A Link ID of any kind, `<kind>:<id>`.
pub fn link_id(at: &str, text: &str) -> Result<(), Refusal> {
    match text.split_once(':') {
        Some((kind, id)) if link_id::is_kind_name(kind) && link_id::is_id(id) => Ok(()),
        _ => refuse(at, format!("{text:?} isn't a Link ID, <kind>:<id>")),
    }
}

/// `const`: exactly `expected`.
/// `word` is one of `words`, the words a choice told by its value takes.
pub fn one_of(at: &str, word: &str, words: &[&str]) -> Result<(), Refusal> {
    if words.contains(&word) {
        Ok(())
    } else {
        refuse(
            at,
            format!("is {word:?}, a whole number or one of {words:?}"),
        )
    }
}

pub fn constant<T: PartialEq + fmt::Debug>(
    at: &str,
    value: &T,
    expected: &T,
) -> Result<(), Refusal> {
    if value == expected {
        Ok(())
    } else {
        refuse(at, format!("is {value:?}, and only {expected:?} is taken"))
    }
}

#[cfg(test)]
mod tests;
