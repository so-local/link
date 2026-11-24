//! The format this build reads, from `format.toml` beside the crate.

use serde::Deserialize;
use std::{collections::BTreeMap, sync::LazyLock};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    pub format: Format,
    pub files: Files,
    pub verbs: Verbs,
    pub route: RouteWords,
    pub core: Core,
    pub rest: RestWords,
}

/// What REST never takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestWords {
    /// Core's shared arguments, and groups, that say where or how a
    /// machine runs an action: a REST call is answered where its service
    /// runs.
    pub never: Vec<String>,
}

/// The bridge words a route's rules depend on.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RouteWords {
    /// The agent's harness, which runs a route's command itself.
    pub harness: String,
    /// Every bridge a route may name.
    pub kinds: Vec<String>,
}

/// The words of the commands core gives every module.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "kebab-case")]
pub struct Core {
    pub program: String,
    pub spec: String,
    pub show: String,
    pub uninstall: String,
    /// The placeholder naming every module's own command in a Next, as
    /// `lnk <module>`.
    pub module_next: String,
    /// The flag every program has, unless its `[program]` declares it.
    pub version: String,
    /// The help flag and command every program has, unless its
    /// `[program]` declares the flag.
    pub help: String,
    /// The hidden root flag an answering module takes, `--link-answer`.
    pub answer: String,
    /// The hidden command a guest service re-enters its program by.
    pub guest: String,
    /// `{kind}` replaced by a spec kind: the docs section of its spec
    /// group and spec show.
    pub spec_docs: String,
    pub module: String,
    pub types: String,
    /// The kind core's half holding its shared shapes declares.
    pub shapes: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Format {
    pub schema: u32,
    pub dialect: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Files {
    pub module: String,
    pub actions: String,
    pub types: String,
    pub variables: String,
    pub schema: String,
    /// Another spec kind's versions, `{kind}` its kind.
    pub kind_schema: String,
    pub max_bytes: u64,
    pub max_depth: usize,
}

/// What an action's output is when its verb implies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Implied {
    /// The resource its ID names.
    Resource,
    /// A list of the resource.
    List,
    /// The resource's ID.
    Id,
    /// Nothing: a check or a run names its own output.
    None,
}

/// A fixed verb: the output it implies and its effect.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fixed {
    pub output: Implied,
    pub effect: crate::Effect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Verbs {
    pub fixed: BTreeMap<String, Fixed>,
    pub banned: BTreeMap<String, String>,
}

pub(crate) static SETTINGS: LazyLock<Settings> = LazyLock::new(|| {
    let settings: Settings =
        toml::from_str(include_str!("../format.toml")).expect("the built-in declaration format");
    assert!(settings.files.max_bytes > 0 && settings.files.max_depth > 0);
    assert!(
        settings
            .verbs
            .banned
            .keys()
            .all(|word| !settings.verbs.fixed.contains_key(word))
    );
    settings
});

/// A spec kind still read by a hand-written type.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldSpec {
    /// The Rust type its generated binding reads.
    #[serde(rename = "type")]
    pub rust: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Held {
    pub specs: BTreeMap<String, HeldSpec>,
}

/// The spec kinds still read by hand-written types (`held.toml`).
pub(crate) static HELD: LazyLock<Held> =
    LazyLock::new(|| toml::from_str(include_str!("../held.toml")).expect("the built-in held list"));

/// Core's shared runtime arguments, as `shared.toml` declares them.
pub(crate) static SHARED: LazyLock<toml::Table> = LazyLock::new(|| {
    toml::from_str(include_str!("../shared.toml")).expect("the built-in shared arguments")
});

#[cfg(test)]
mod tests {
    #[test]
    fn the_shipped_format_reads() {
        assert_eq!(super::SETTINGS.format.schema, 1);
        assert!(!super::SHARED.is_empty());
    }
}
