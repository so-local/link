//! Where and how each surface is written, from `surfaces.toml` beside the
//! crate.

use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    pub generator: Generator,
    pub openapi: OpenApi,
    pub surfaces: Surfaces,
    pub typescript: TypeScript,
    pub rust: Rust,
    pub app: App,
    pub tools: Tools,
    pub reference: Reference,
    pub completion: Completion,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Generator {
    pub marker: String,
    pub json_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct OpenApi {
    pub version: String,
    pub title: String,
    pub api_version: String,
    pub prefix: String,
    pub file: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Surfaces {
    pub cli_only: Vec<String>,
    pub person_only: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TypeScript {
    pub types: String,
    pub client: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rust {
    pub types: String,
    pub client: String,
    pub handler: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct App {
    pub extension: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Tools {
    pub file: String,
    pub separator: String,
    pub max_name: usize,
    pub spec: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Reference {
    pub commands: String,
    pub specs: String,
    pub hub: String,
    pub hub_hook: String,
    /// The folders whose next folder names a module's domain, as
    /// `modules/tooling/bundle`'s `tooling`.
    pub domains: Vec<String>,
    /// The hook under each domain's hub title.
    pub domain_hook: String,
    pub synopsis: String,
    pub variables: String,
    /// A word of a name a heading writes out in full, by the word.
    pub spelled_out: std::collections::BTreeMap<String, String>,
    /// A word of a name that is an acronym, which a heading writes in
    /// capitals, by the word in lowercase.
    pub acronyms: std::collections::BTreeSet<String>,
}

pub(crate) static SETTINGS: LazyLock<Settings> = LazyLock::new(|| {
    let settings: Settings =
        toml::from_str(include_str!("../surfaces.toml")).expect("the built-in surface settings");
    assert!(
        !settings.generator.marker.contains('\n'),
        "the marker is one line"
    );
    assert!(
        settings.tools.separator.chars().all(|c| c == '_'),
        "a tool name's separator is no character an action's name holds"
    );
    settings
});

#[cfg(test)]
mod tests {
    #[test]
    fn the_shipped_settings_read() {
        assert!(super::SETTINGS.openapi.prefix.starts_with('/'));
        assert!(!super::SETTINGS.surfaces.cli_only.is_empty());
    }

    /// Each acronym is a name's word as the command line types it, in
    /// lowercase, and no word is both spelled out and an acronym.
    #[test]
    fn acronyms_are_words_of_names() {
        let reference = &super::SETTINGS.reference;
        assert!(reference.acronyms.contains("mcp"));
        for acronym in &reference.acronyms {
            assert!(
                !acronym.is_empty() && acronym.chars().all(|c| c.is_ascii_lowercase()),
                "{acronym}"
            );
            assert!(!reference.spelled_out.contains_key(acronym), "{acronym}");
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Completion {
    pub bash: String,
    pub zsh: String,
    pub bash_comment: String,
    pub zsh_comment: String,
    pub spec_help: String,
    pub group_help: String,
}
