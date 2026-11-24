//! What Link Web, `lnk-web-tool`, is called with and answers, generated
//! from the module's declarations (`actions.toml` and `types.toml`): a
//! model's [`WebToolFetchArgs`] and [`WebToolSearchArgs`], and a fetched
//! [`Page`] and search [`Results`], each [`Hit`] a page found. The
//! program reads its calls and answers with these types, and the agent
//! module's `web fetch` and `web search` actions check their arguments
//! and read their answers through the same ones. [`WebToolServeArgs`] is
//! `serve`'s line, which the agent module builds.

/// The calls a model makes of the tool, as its declarations render them:
/// the template its program fills ([`link_actions::tools::offer`]).
pub const TOOLS: &str = include_str!(concat!(env!("OUT_DIR"), "/tools.json"));

/// Every call the tool declares, by the name a model calls it.
pub fn calls() -> anyhow::Result<Vec<String>> {
    Ok(link_actions::tools::Template::parse(TOOLS)?.calls())
}

/// The calls the tool offers with a search key (`search`) or without.
pub fn calls_with(search: bool) -> anyhow::Result<Vec<String>> {
    let conditions = search.then_some("search").into_iter().collect();
    let template = link_actions::tools::Template::parse(TOOLS)?;
    Ok(template
        .offered(&conditions)
        .map(|tool| tool.name.clone())
        .collect())
}

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

#[cfg(test)]
link_test_kit::program!(unit);
