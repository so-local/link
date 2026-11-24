//! Model IDs are provider IDs, not display names. Facts can suggest an ID
//! but cannot establish whether an account has access to it.

use super::providers::Declared;

/// A name accepted by the CLI. Reject a known misspelling without silently
/// changing it; IDs of models released after Link remain usable.
pub fn name(input: &str) -> Result<String, String> {
    if !super::show::plain_name(input) {
        return Err("give a plain model ID".into());
    }
    let corrected = input.replace('.', "-");
    let models = super::link();
    if corrected != input
        && !models.model.contains_key(input)
        && models.model.contains_key(&corrected)
    {
        return Err(format!(
            "{input} is not a model ID; did you mean {corrected}?"
        ));
    }
    Ok(input.into())
}

/// Refuse an unavailable ID before any selection is persisted. The offered
/// IDs come from the selected runtime or provider, not the numeric facts.
pub fn offered(input: &str, models: &[String]) -> Result<(), String> {
    name(input)?;
    if models.iter().any(|model| model == input) {
        return Ok(());
    }
    Err(format!(
        "{input} is not offered by this provider; choose an exact ID from its model list"
    ))
}

/// Who serves a model, as the first part of its name says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// A model runtime on this machine (`local/qwen3:8b`).
    Local,
    /// Anthropic's API (`anthropic/claude-sonnet-5-5`).
    Anthropic,
    /// OpenAI's API (`openai/gpt-6.1-sol`).
    OpenAi,
    /// OpenRouter, for every other vendor (`openrouter/deepseek/deepseek-v4`).
    OpenRouter,
    /// A provider an installed module adds (`<name>/<id>`), at the address
    /// its declaration says ([`super::providers`]): by its name, and its
    /// title for what Link says.
    Declared {
        name: &'static str,
        title: &'static str,
    },
}

impl Provider {
    /// The first part of a full model name, and a hosted provider's id.
    pub fn id(self) -> &'static str {
        match self {
            Provider::Local => "local",
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
            Provider::OpenRouter => "openrouter",
            Provider::Declared { name, .. } => name,
        }
    }

    /// Its name, for what Link says.
    pub fn title(self) -> &'static str {
        match self {
            Provider::Local => "this machine",
            Provider::Anthropic => "Anthropic",
            Provider::OpenAi => "OpenAI",
            Provider::OpenRouter => "OpenRouter",
            Provider::Declared { title, .. } => title,
        }
    }

    /// The provider a saved model's `provider` is: `ollama` and `local`
    /// are both this machine's.
    pub fn of_saved(provider: &str) -> Option<Provider> {
        match provider {
            "local" | "ollama" => Some(Provider::Local),
            "anthropic" => Some(Provider::Anthropic),
            "openai" => Some(Provider::OpenAi),
            "openrouter" => Some(Provider::OpenRouter),
            other => super::providers::named(other).map(Provider::declared),
        }
    }

    /// A provider a module adds, by its name and declaration.
    fn declared((name, declared): (&'static str, &'static Declared)) -> Provider {
        Provider::Declared {
            name,
            title: declared.title.as_str(),
        }
    }
}

/// How a name that only a module provides is looked up: its name and
/// declaration, when an installed module adds it.
type Lookup = fn(&str) -> Option<(&'static str, &'static Declared)>;

/// A model and where it's sent. Its full name starts with who serves it:
/// `local/`, `anthropic/`, `openai/`, `openrouter/` or a provider an
/// installed module adds. A bare name goes to
/// a model of that name running here first, else to whoever owns it,
/// never OpenRouter by default: `claude-...` to Anthropic and `gpt-...`
/// (or an `o3`-like name) to OpenAI. A provider's name in other letters
/// (`Anthropic/`) is refused, never taken as another vendor's. A bare
/// name with no owner Link knows goes to OpenRouter, as does another
/// vendor's (`deepseek/deepseek-v4`). A name only a module provides goes
/// to the provider its module declares, and is refused when no installed
/// module declares it, never sent to OpenRouter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub provider: Provider,
    /// The id its provider knows it by: `qwen3:8b`, `gpt-6.1-sol`, or
    /// OpenRouter's `deepseek/deepseek-v4`.
    pub id: String,
    /// A bare name: a model of that name running here takes it
    /// ([`Route::here`]), else `provider`, its owner or OpenRouter.
    pub here_first: bool,
}

impl Route {
    /// `input`, a full name or a bare one Link can place.
    pub fn parse(input: &str) -> Result<Route, String> {
        Route::parse_with(input, super::providers::named)
    }

    /// [`Route::parse`], with a name only a module provides looked up by
    /// `named`.
    fn parse_with(input: &str, named: Lookup) -> Result<Route, String> {
        let input = input.trim();
        let route = match input.split_once('/') {
            Some(("local", id)) => Route::new(Provider::Local, id),
            Some(("anthropic", id)) => Route::new(Provider::Anthropic, id),
            Some(("openai", id)) => Route::new(Provider::OpenAi, id),
            Some((first, id)) if super::providers::is_provided(first) => match named(first) {
                Some(declared) => Route::new(Provider::declared(declared), id),
                None => return Err(format!("no installed module provides {first} models")),
            },
            // OpenRouter's own models keep its name (`openrouter/auto`).
            Some(("openrouter", id)) if !id.contains('/') => {
                Route::new(Provider::OpenRouter, input)
            }
            Some(("openrouter", id)) => Route::new(Provider::OpenRouter, id),
            Some((first, id)) => {
                unsaid(first, id)?;
                Route::new(Provider::OpenRouter, input)
            }
            None => Route {
                here_first: true,
                ..Route::new(owner(input).unwrap_or(Provider::OpenRouter), input)
            },
        };
        if route.id.is_empty() {
            return Err(format!("{input} names no model after its provider"));
        }
        if matches!(
            route.provider,
            Provider::Anthropic | Provider::OpenAi | Provider::Declared { .. }
        ) && route.id.contains('/')
        {
            return Err(format!(
                "{} names its models without a slash: say openrouter/{} for OpenRouter's",
                route.provider.title(),
                route.id
            ));
        }
        // A hosted provider's id is checked for known misspellings; a
        // model here, or a bare name with no owner, only for its shape.
        let hosted = match route.provider {
            Provider::Local => false,
            Provider::OpenRouter => !route.here_first,
            Provider::Anthropic | Provider::OpenAi | Provider::Declared { .. } => true,
        };
        if hosted {
            name(&route.id)?;
        } else if !super::show::plain_name(&route.id) {
            return Err("give a plain model ID".into());
        }
        Ok(route)
    }

    /// The route of a model Link saved: its provider, and its id there.
    pub fn of_saved(provider: &str, id: &str) -> Option<Route> {
        Some(Route::new(Provider::of_saved(provider)?, id))
    }

    fn new(provider: Provider, id: &str) -> Route {
        Route {
            provider,
            id: id.to_string(),
            here_first: false,
        }
    }

    /// It sent to the model of its name running here: a bare name's,
    /// when one runs.
    pub fn here(self) -> Route {
        Route {
            provider: Provider::Local,
            here_first: false,
            ..self
        }
    }

    /// Whether a bare name has an owner Link knows: then only a model of
    /// exactly that name here takes it, never one it merely starts.
    pub fn owned(&self) -> bool {
        self.here_first && self.provider != Provider::OpenRouter
    }

    /// Whether it may be served by `provider` (a saved model's: `ollama`
    /// and `local` are both here): its own, or here for a bare name.
    pub fn may_be(&self, provider: &str) -> bool {
        let saved = Provider::of_saved(provider);
        saved == Some(self.provider) || (self.here_first && saved == Some(Provider::Local))
    }

    /// Whether the name said its provider in so many words: false for a
    /// bare name and for another vendor's, which goes through OpenRouter.
    pub fn said(input: &str) -> bool {
        match input.trim().split_once('/') {
            Some(("local" | "anthropic" | "openai" | "openrouter", _)) => true,
            Some((first, _)) => super::providers::is_provided(first),
            None => false,
        }
    }
}

/// Its full name: `local/qwen3:8b`, `openrouter/deepseek/deepseek-v4`.
impl std::fmt::Display for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.provider == Provider::OpenRouter && self.id.starts_with("openrouter/") {
            return f.write_str(&self.id);
        }
        write!(f, "{}/{}", self.provider.id(), self.id)
    }
}

/// Refuses a first part that names a provider in other letters
/// (`Anthropic/`), which would otherwise be taken as another vendor's,
/// through OpenRouter: a provider is named in lowercase alone.
fn unsaid(first: &str, id: &str) -> Result<(), String> {
    let lower = first.to_ascii_lowercase();
    if super::providers::is_provided(&lower) {
        return Err(format!(
            "{first}/{id} names {lower} in other letters: say {lower}/{id}"
        ));
    }
    match PROVIDERS.iter().find(|p| p.id() == lower) {
        Some(provider) => Err(format!(
            "{first}/{id} names {} in other letters: say {}/{id}",
            provider.title(),
            provider.id()
        )),
        None => Ok(()),
    }
}

/// Every provider Link includes, as a full name's first part says it.
const PROVIDERS: [Provider; 4] = [
    Provider::Local,
    Provider::Anthropic,
    Provider::OpenAi,
    Provider::OpenRouter,
];

/// The first party a model's own name says it's of, as a bare name's
/// owner: `claude-...` Anthropic's, `gpt-...`, `chatgpt-...` and
/// `o3`-like names OpenAI's.
pub fn owner(name: &str) -> Option<Provider> {
    let name = name.to_ascii_lowercase();
    let o_series = name.strip_prefix('o').is_some_and(|rest| {
        rest.split('-')
            .next()
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    });
    if name.starts_with("claude-") {
        Some(Provider::Anthropic)
    } else if name.starts_with("gpt-") || name.starts_with("chatgpt-") || o_series {
        Some(Provider::OpenAi)
    } else {
        None
    }
}

/// A name `--model` takes: one [`Route::parse`] places.
pub fn routed(input: &str) -> Result<String, String> {
    Route::parse(input).map(|_| input.trim().to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn provider_ids_are_exact_and_not_limited_to_shipped_facts() {
        assert!(
            super::name("claude-haiku-5.5")
                .unwrap_err()
                .contains("claude-haiku-5-5")
        );
        assert!(super::name("anthropic/claude-haiku-5.5").is_ok());
        assert!(super::offered("future-model", &["future-model".into()]).is_ok());
        assert!(super::offered("future-model-typo", &["future-model".into()]).is_err());
        assert!(super::name("../secret").is_err());
    }

    use super::{Provider, Route};

    fn route(input: &str) -> (Provider, String, String) {
        let r = Route::parse_with(input, example).unwrap();
        (r.provider, r.id.clone(), r.to_string())
    }

    /// A provider a module adds, as a fixture declares it under the name
    /// only a module provides.
    fn example(name: &str) -> Option<(&'static str, &'static super::Declared)> {
        static EXAMPLE: std::sync::LazyLock<super::Declared> =
            std::sync::LazyLock::new(|| super::Declared {
                title: "Example Models".into(),
                base_url: "https://models.gw.example/v1".into(),
                speaks: "openai".into(),
                key_prefix: "ex_key_".into(),
            });
        (name == "link").then(|| ("link", &*EXAMPLE))
    }

    const EXAMPLE: Provider = Provider::Declared {
        name: "link",
        title: "Example Models",
    };

    /// A provider a module adds is said by its name, never by a bare one;
    /// with no installed module adding it, a model named by it is refused
    /// by name, never sent to OpenRouter.
    #[test]
    fn a_provided_name_goes_to_its_module_or_is_refused() {
        assert_eq!(
            route("link/auto"),
            (EXAMPLE, "auto".into(), "link/auto".into())
        );
        assert!(Route::said("link/auto"));
        assert_eq!(EXAMPLE.title(), "Example Models");
        assert!(Route::parse_with("link/some/thing", example).is_err());
        assert_ne!(
            Route::parse_with("auto", example).unwrap().provider,
            EXAMPLE
        );
        assert_eq!(
            Route::parse_with("link/auto", |_| None).unwrap_err(),
            "no installed module provides link models"
        );
    }

    #[test]
    fn a_full_name_says_who_serves_it() {
        assert_eq!(
            route("local/qwen3:8b"),
            (Provider::Local, "qwen3:8b".into(), "local/qwen3:8b".into())
        );
        assert_eq!(
            route("local/hf.co/org/model:q4"),
            (
                Provider::Local,
                "hf.co/org/model:q4".into(),
                "local/hf.co/org/model:q4".into()
            )
        );
        assert_eq!(
            route("anthropic/claude-sonnet-5-5"),
            (
                Provider::Anthropic,
                "claude-sonnet-5-5".into(),
                "anthropic/claude-sonnet-5-5".into()
            )
        );
        assert_eq!(
            route("openai/gpt-6.1-sol"),
            (
                Provider::OpenAi,
                "gpt-6.1-sol".into(),
                "openai/gpt-6.1-sol".into()
            )
        );
        assert_eq!(
            route("openrouter/anthropic/claude-haiku-5.5"),
            (
                Provider::OpenRouter,
                "anthropic/claude-haiku-5.5".into(),
                "openrouter/anthropic/claude-haiku-5.5".into()
            )
        );
        assert_eq!(
            route("openrouter/auto"),
            (
                Provider::OpenRouter,
                "openrouter/auto".into(),
                "openrouter/auto".into()
            )
        );
    }

    #[test]
    fn a_bare_name_goes_here_then_to_its_owner_then_openrouter() {
        assert_eq!(route("claude-sonnet-5-5").2, "anthropic/claude-sonnet-5-5");
        assert_eq!(route("gpt-6.1-sol").2, "openai/gpt-6.1-sol");
        assert_eq!(route("o4-mini").2, "openai/o4-mini");
        // Every bare name looks here first.
        let gpt = Route::parse("gpt-oss:20b").unwrap();
        assert!(gpt.here_first && gpt.owned());
        assert!(gpt.may_be("ollama") && gpt.may_be("openai") && !gpt.may_be("openrouter"));
        assert_eq!(gpt.clone().here().to_string(), "local/gpt-oss:20b");
        // No owner Link knows: here, else OpenRouter.
        let qwen = Route::parse("qwen3:8b").unwrap();
        assert!(qwen.here_first && !qwen.owned());
        assert_eq!(qwen.to_string(), "openrouter/qwen3:8b");
        assert!(qwen.may_be("ollama") && qwen.may_be("openrouter") && !qwen.may_be("openai"));
        // Said in so many words, it goes where it says.
        assert!(!Route::parse("openai/gpt-6.1-sol").unwrap().may_be("ollama"));
        assert!(
            !Route::parse("openrouter/qwen3:8b")
                .unwrap()
                .may_be("ollama")
        );
        assert!(!Route::parse("local/qwen3:8b").unwrap().may_be("openrouter"));
        // A misspelling of an owner's model is still refused.
        assert!(Route::parse("claude-haiku-5.5").is_err());
        assert!(!Route::said("claude-sonnet-5-5"));
        assert!(Route::said("anthropic/claude-sonnet-5-5"));
    }

    #[test]
    fn another_vendor_goes_through_openrouter() {
        assert_eq!(
            route("deepseek/deepseek-v4"),
            (
                Provider::OpenRouter,
                "deepseek/deepseek-v4".into(),
                "openrouter/deepseek/deepseek-v4".into()
            )
        );
        assert!(!Route::said("deepseek/deepseek-v4"));
    }

    #[test]
    fn a_direct_provider_never_takes_a_routers_name() {
        assert!(
            Route::parse("anthropic/claude-haiku-5.5")
                .unwrap_err()
                .contains("claude-haiku-5-5")
        );
        assert!(
            Route::parse("openai/openai/gpt-6-luna")
                .unwrap_err()
                .contains("openrouter/openai/gpt-6-luna")
        );
        assert!(Route::parse("local/").is_err());
        assert!(Route::parse("local/../secret").is_err());
        assert!(Route::parse("").is_err());
        assert!(Route::parse("../secret").is_err());
    }

    /// Every way a name can say its provider: a first party's prefix, its
    /// model through OpenRouter, a provider a module adds, this machine,
    /// and a bare name to its owner.
    #[test]
    fn each_prefix_goes_to_the_provider_it_names() {
        for (input, provider, id, full) in [
            (
                "anthropic/claude-sonnet-5-5",
                Provider::Anthropic,
                "claude-sonnet-5-5",
                "anthropic/claude-sonnet-5-5",
            ),
            (
                "openai/gpt-6-luna",
                Provider::OpenAi,
                "gpt-6-luna",
                "openai/gpt-6-luna",
            ),
            (
                "claude-haiku-5-5",
                Provider::Anthropic,
                "claude-haiku-5-5",
                "anthropic/claude-haiku-5-5",
            ),
            (
                "gpt-6-luna",
                Provider::OpenAi,
                "gpt-6-luna",
                "openai/gpt-6-luna",
            ),
            ("o4-mini", Provider::OpenAi, "o4-mini", "openai/o4-mini"),
            (
                "openrouter/anthropic/claude-haiku-5.5",
                Provider::OpenRouter,
                "anthropic/claude-haiku-5.5",
                "openrouter/anthropic/claude-haiku-5.5",
            ),
            (
                "openrouter/openai/gpt-6-luna",
                Provider::OpenRouter,
                "openai/gpt-6-luna",
                "openrouter/openai/gpt-6-luna",
            ),
            (
                "openrouter/auto",
                Provider::OpenRouter,
                "openrouter/auto",
                "openrouter/auto",
            ),
            ("link/auto", EXAMPLE, "auto", "link/auto"),
            (
                "local/qwen3:8b",
                Provider::Local,
                "qwen3:8b",
                "local/qwen3:8b",
            ),
        ] {
            assert_eq!(route(input), (provider, id.into(), full.into()), "{input}");
        }
    }

    /// A provider's name in other letters is refused naming what to say:
    /// never taken as another vendor's, through OpenRouter.
    #[test]
    fn a_provider_misspelled_is_refused_never_sent_to_openrouter() {
        for (input, says) in [
            (
                "Anthropic/claude-haiku-5-5",
                "say anthropic/claude-haiku-5-5",
            ),
            (
                "ANTHROPIC/claude-haiku-5-5",
                "say anthropic/claude-haiku-5-5",
            ),
            ("OpenAI/gpt-6-luna", "say openai/gpt-6-luna"),
            ("OpenRouter/auto", "say openrouter/auto"),
            ("Local/qwen3:8b", "say local/qwen3:8b"),
            ("Link/auto", "say link/auto"),
        ] {
            let refused = Route::parse(input).unwrap_err();
            assert!(refused.contains(says), "{input}: {refused}");
        }
    }

    /// Whatever its letters, a name whose first part says Anthropic or
    /// OpenAI never reaches OpenRouter; nor does a bare name of theirs.
    #[test]
    fn no_first_party_name_reaches_openrouter_unasked() {
        fn casings(word: &str) -> Vec<String> {
            let letters: Vec<char> = word.chars().collect();
            (0..1u32 << letters.len())
                .map(|mask| {
                    letters
                        .iter()
                        .enumerate()
                        .map(|(i, c)| match mask >> i & 1 {
                            1 => c.to_ascii_uppercase(),
                            _ => *c,
                        })
                        .collect()
                })
                .collect()
        }
        let mut firsts = Vec::new();
        for word in ["anthropic", "openai"] {
            firsts.extend(casings(word));
        }
        for first in &firsts {
            for model in ["claude-sonnet-5-5", "gpt-6-luna", "x"] {
                let input = format!("{first}/{model}");
                if let Ok(route) = Route::parse(&input) {
                    assert_ne!(route.provider, Provider::OpenRouter, "{input}");
                }
            }
        }
        for bare in casings("claude-x").into_iter().chain(casings("gpt-x")) {
            let route = Route::parse(&bare).unwrap();
            assert_ne!(route.provider, Provider::OpenRouter, "{bare}");
        }
    }

    #[test]
    fn a_saved_model_reads_back_as_its_route() {
        assert_eq!(
            Route::of_saved("ollama", "qwen3:8b").unwrap().to_string(),
            "local/qwen3:8b"
        );
        assert_eq!(
            Route::of_saved("openrouter", "anthropic/claude-haiku-5.5")
                .unwrap()
                .to_string(),
            "openrouter/anthropic/claude-haiku-5.5"
        );
        assert_eq!(Route::of_saved("elsewhere", "x"), None);
    }
}
