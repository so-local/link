//! The providers an installed module adds, each at an address it declares
//! and speaking another provider's API there: asked once of the modules
//! that answer [`QUESTION`], and kept for the process. Only a name
//! `providers.toml` says a module provides is asked about, so a model of
//! any other provider never waits on the question.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Which names only a module provides, as `providers.toml` says.
const PROVIDED: &str = include_str!("providers.toml");

/// `model.providers`: the providers an installed module adds.
pub const QUESTION: link_core::answers::Question<ProvidersAsk, Providers> =
    link_core::answers::Question::new("model.providers", include_str!("providers/question.toml"));

/// What [`QUESTION`] asks: nothing but the question.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvidersAsk {}

/// What a module answers [`QUESTION`]: each provider it adds, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Providers {
    #[serde(default)]
    pub provider: BTreeMap<String, Declared>,
}

/// A provider at an address of its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declared {
    /// Its name, for what Link says.
    pub title: String,
    /// Its API's address, up to and with its `/v1`.
    pub base_url: String,
    /// Whose API it speaks there: a provider of the model route's
    /// ([`super::route::PROVIDERS`]).
    pub speaks: String,
    /// What its keys start with.
    pub key_prefix: String,
}

impl Declared {
    /// An error unless it's at an HTTPS address, speaks a provider the
    /// model route knows, and has a key prefix of its own.
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.base_url.starts_with("https://"),
            "its address isn't HTTPS"
        );
        anyhow::ensure!(
            super::route::PROVIDERS.contains(&self.speaks.as_str()),
            "it speaks {}, which no model route knows",
            link_format::clean(&self.speaks)
        );
        anyhow::ensure!(
            !self.key_prefix.is_empty() && !self.key_prefix.starts_with("sk-"),
            "its keys have no prefix of their own"
        );
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    provided: Vec<String>,
}

/// The names only a module provides.
fn provided() -> &'static [String] {
    static PROVIDED_NAMES: OnceLock<Vec<String>> = OnceLock::new();
    PROVIDED_NAMES.get_or_init(|| {
        toml::from_str::<File>(PROVIDED)
            .expect("providers.toml is checked by its test")
            .provided
    })
}

/// Whether `name` is one only a module provides: a model named by it goes
/// to that module's provider, or is refused, never to OpenRouter.
pub fn is_provided(name: &str) -> bool {
    provided().iter().any(|p| p == name)
}

/// The providers this process knows, asked once.
static KNOWN: OnceLock<BTreeMap<String, Declared>> = OnceLock::new();

/// The providers `answers` add: each a name only a module provides, its
/// declaration checked, the first module's kept where two declare one.
fn adding(answers: impl IntoIterator<Item = Providers>) -> BTreeMap<String, Declared> {
    let mut known = BTreeMap::new();
    for answer in answers {
        for (name, declared) in answer.provider {
            if is_provided(&name) && declared.check().is_ok() {
                known.entry(name).or_insert(declared);
            }
        }
    }
    known
}

fn known() -> &'static BTreeMap<String, Declared> {
    KNOWN.get_or_init(|| {
        adding(
            QUESTION
                .ask(&ProvidersAsk {})
                .into_iter()
                .filter_map(|answer| answer.reply.ok()),
        )
    })
}

/// The providers this process knows, given instead of asked: for a
/// process told them another way, such as a test's fixture. False when
/// they're known already.
pub fn declare(providers: Providers) -> bool {
    KNOWN.set(adding([providers])).is_ok()
}

/// `provider`'s name and declaration, when it's one an installed module
/// adds.
pub fn named(provider: &str) -> Option<(&'static str, &'static Declared)> {
    if !is_provided(provider) {
        return None;
    }
    known()
        .get_key_value(provider)
        .map(|(name, declared)| (name.as_str(), declared))
}

/// `provider`'s declaration, when it's one an installed module adds.
pub fn declared(provider: &str) -> Option<&'static Declared> {
    named(provider).map(|(_, declared)| declared)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> Declared {
        Declared {
            title: "Example Models".into(),
            base_url: "https://models.gw.example/v1".into(),
            speaks: "openai".into(),
            key_prefix: "ex_key_".into(),
        }
    }

    /// The names a module provides are none the model route knows, and a
    /// module adds only those, each at an HTTPS address, speaking a
    /// provider the route knows, with a key prefix of its own.
    #[test]
    fn a_module_adds_only_a_provided_name_well_declared() {
        assert!(!provided().is_empty());
        for name in provided() {
            assert!(
                !super::super::route::PROVIDERS.contains(&name.as_str()),
                "{name}"
            );
        }
        let name = provided()[0].clone();
        let answer = |declared: Declared| Providers {
            provider: BTreeMap::from([(name.clone(), declared), ("openai".into(), example())]),
        };
        let added = adding([answer(example())]);
        assert_eq!(added.keys().collect::<Vec<_>>(), [&name]);
        for wrong in [
            Declared {
                base_url: "http://models.gw.example/v1".into(),
                ..example()
            },
            Declared {
                speaks: "ollama".into(),
                ..example()
            },
            Declared {
                key_prefix: "sk-".into(),
                ..example()
            },
        ] {
            assert!(adding([answer(wrong)]).is_empty());
        }
        assert!(!is_provided("openai"));
    }

    /// The question and its answer read back as written.
    #[test]
    fn the_providers_question_reads_back() {
        let text = serde_json::to_string(&QUESTION.asked(&ProvidersAsk {})).unwrap();
        assert_eq!(text, r#"{"protocol":1,"question":{}}"#);
        let answer = Providers {
            provider: BTreeMap::from([("gw".into(), example())]),
        };
        let text = serde_json::to_string(&QUESTION.answered(&answer)).unwrap();
        let back: link_core::answers::Answered<Providers> = serde_json::from_str(&text).unwrap();
        assert_eq!(QUESTION.answer_of("example", back).unwrap(), answer);
    }
}
