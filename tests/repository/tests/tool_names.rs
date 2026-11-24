//! The tool names a model is told are ones it's offered: the rule is
//! `link_repository_check::tool_names`.

use std::path::Path;

#[test]
fn every_tool_name_a_model_is_told_is_one_an_agent_may_call() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let actions = root.join("modules/agents/agent/actions.toml");
    let problems = link_repository_check::tool_names::problems(&root, &["."], &actions);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
