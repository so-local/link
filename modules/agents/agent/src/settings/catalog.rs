//! The harnesses and memory engines Link knows: their names, modules and
//! runtimes, and which reach a hosted model through a proxy.

/// The harness `lnk agent start` installs first: Link Harness.
pub const DEFAULT_HARNESS: &str = "link";

/// Whether harness `name` reaches its hosted model through its proxy's
/// address, so the proxy can hold the key and the harness only a
/// placeholder, unless the agent says otherwise: Link Harness, built to
/// hold no keys, and any other whose adapter says so (`lnk-<name>
/// contract`, asked once a process while its program stays the same).
pub fn takes_proxy(name: &str) -> anyhow::Result<bool> {
    if name == DEFAULT_HARNESS {
        return Ok(true);
    }
    match crate::adapter::Adapter::find(name) {
        Ok(adapter) => Ok(adapter.contract()?.proxy),
        Err(_) => Ok(false),
    }
}

/// Harnesses Link comes with: the name you type (`lnk agent harness set link`), its
/// adapter's module (`lnk-<module>`), and what it is. Any other
/// `lnk-<name>` adapter on the PATH works too, by its own name: one from
/// outside Link, added with `lnk module add --url <url>`.
pub const KNOWN: &[(&str, &str, &str)] = &[
    (
        "link",
        "harness",
        "Link Harness: small, answers you and remembers the thread",
    ),
    (
        "mini",
        "mini-harness",
        "Mini Harness: text, tools and saved conversations; no skills, persona or compaction",
    ),
    (
        "openclaw",
        "openclaw",
        "OpenClaw, the personal AI assistant (openclaw.ai)",
    ),
    ("hermes", "hermes", "Hermes Agent by Nous Research"),
    (
        "deepseek",
        "deepseek",
        "DeepSeek Harness, DeepSeek's open-source agent (dsh)",
    ),
];

/// What Link's harnesses that download a runtime of their own download,
/// the first time each agent's runs: asked first.
pub const RUNTIMES: &[(&str, &str)] = &[
    ("openclaw", "Node and OpenClaw's packages"),
    ("hermes", "Python and Hermes's repository and packages"),
    ("deepseek", "DeepSeek Harness's packages, about 800 MB"),
];

/// Every harness there is to use: Link's ([`KNOWN`]), then additional
/// harnesses this build declares or added from outside Link.
pub fn harnesses() -> Vec<(String, String)> {
    harnesses_in(link_core::modules::list())
}

fn harnesses_in(listed: Vec<link_core::modules::Listed>) -> Vec<(String, String)> {
    KNOWN
        .iter()
        .map(|(name, _, about)| (name.to_string(), about.to_string()))
        .chain(
            listed
                .into_iter()
                .filter(|p| p.kind == Some(link_core::modules::Kind::Harness) || p.source.is_some())
                .filter(|p| !KNOWN.iter().any(|(_, module, _)| *module == p.name))
                .map(|p| {
                    let about = p.description.unwrap_or_else(|| {
                        format!("from outside Link: {}", p.source.unwrap_or_default())
                    });
                    (p.name, about)
                }),
        )
        .collect()
}

/// Whether this build supplies the adapter, including added first-party declarations.
pub fn known_harness(name: &str) -> bool {
    KNOWN.iter().any(|(known, _, _)| *known == name)
        || declared_harness(name, &link_core::modules::list())
}

fn declared_harness(name: &str, listed: &[link_core::modules::Listed]) -> bool {
    listed.iter().any(|p| {
        p.name == name && p.source.is_none() && p.kind == Some(link_core::modules::Kind::Harness)
    })
}

/// What a fresh runtime install asks to download, including an additional harness.
pub fn runtime_of(name: &str) -> Option<&'static str> {
    RUNTIMES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, what)| *what)
        .or_else(|| {
            (!KNOWN.iter().any(|(known, _, _)| *known == name)
                && declared_harness(name, &link_core::modules::list()))
            .then_some("its runtime and packages")
        })
}

/// Where the known harnesses from outside Link are listed, for messages.
pub const MORE_HARNESSES: &str = link_core::docs!("agents/harnesses/use#use-another-harness");

/// Memory engines Link knows: the name you type (`lnk agent memory start
/// link`), its adapter's module (`lnk-<module>`), and what it is. Any
/// other `lnk-<name>` answering `link_memory_tool_contract` works too.
pub const MEMORIES: &[(&str, &str, &str)] = &[(
    "link",
    "memory-tool",
    "Link Memory: full-text search over your agent's files, keyless and local",
)];

/// The engine `lnk agent memory start` picks when none is named.
pub const DEFAULT_MEMORY: &str = "link";

/// The module of memory engine `name`: its line in [`MEMORIES`], else
/// its own name.
pub fn memory_module_of(name: &str) -> &str {
    MEMORIES
        .iter()
        .find(|(known, _, _)| *known == name)
        .map_or(name, |(_, module, _)| module)
}

/// The module of harness `name`: its line in [`KNOWN`], else its own
/// name (an adapter from outside Link, `lnk-<name>`).
pub fn module_of(name: &str) -> &str {
    KNOWN
        .iter()
        .find(|(known, _, _)| *known == name)
        .map_or(name, |(_, module, _)| module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_core::modules::{Kind, Listed};

    #[test]
    fn additional_build_harnesses_are_listed_and_installable_by_declaration() {
        let listed = vec![
            Listed {
                name: "harness".into(),
                kind: Some(Kind::Harness),
                ..Default::default()
            },
            Listed {
                name: "mini-harness".into(),
                kind: Some(Kind::Harness),
                ..Default::default()
            },
            Listed {
                name: "example".into(),
                kind: Some(Kind::Harness),
                description: Some("An additional harness".into()),
                ..Default::default()
            },
            Listed {
                name: "other".into(),
                kind: Some(Kind::Tool),
                ..Default::default()
            },
            Listed {
                name: "outside".into(),
                source: Some("https://example.org/harness".into()),
                ..Default::default()
            },
        ];
        assert!(declared_harness("example", &listed));
        assert!(!declared_harness("other", &listed));
        assert!(!declared_harness("outside", &listed));
        let harnesses = harnesses_in(listed);
        assert!(
            !harnesses
                .iter()
                .any(|(name, _)| name == "harness" || name == "mini-harness")
        );
        assert!(harnesses.contains(&("example".into(), "An additional harness".into())));
        assert!(!harnesses.iter().any(|(name, _)| name == "other"));
        assert!(harnesses.contains(&(
            "outside".into(),
            "from outside Link: https://example.org/harness".into()
        )));
    }
}
