//! Memory adapter and shared-session protocol.

/// The calls a model makes of the tool, as its declarations render them:
/// the template its program fills ([`link_actions::tools::offer`]).
pub const TOOLS: &str = include_str!(concat!(env!("OUT_DIR"), "/tools.json"));

/// Every call the tool declares, by the name a model calls it.
pub fn calls() -> anyhow::Result<Vec<String>> {
    Ok(link_actions::tools::Template::parse(TOOLS)?.calls())
}

/// The calls the tool declares as only reading.
pub fn reads() -> anyhow::Result<Vec<String>> {
    Ok(link_actions::tools::Template::parse(TOOLS)?.reads())
}

pub use link_agent_identity::owner as identity;
pub use link_agent_spec as agent_spec;
pub use link_core::*;
pub use link_sandbox_binding as bridge;
pub use link_store_contract::storage;
mod memory;
pub use memory::*;

/// The memory protocol, generated from this crate's declarations
/// (`actions.toml` and `types.toml`): each command's runtime arguments,
/// with the command line that gives them (`argv()`), which `lnk agent`
/// builds and an engine's generated parser reads, and the shapes an engine
/// prints and reads ([`declared::Launch`], [`declared::Agents`]).
pub mod declared {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}

/// Link Memory's own commands (`lnk-memory-tool`'s `actions.toml`): each
/// one's runtime arguments, with the command line that gives them
/// (`argv()`), which a machine builds and its parser reads.
pub mod tool {
    include!(concat!(env!("OUT_DIR"), "/tool.rs"));
}

// The agents file `serve-many` reads, written by Link for each start,
// naming its schema.
link_files::specification!(declared::Agents, include_str!("agents.schema.toml"));

mod checks;

/// Where a model is served comes with the model it serves: an address
/// alone names nothing to rank by. Every command that takes one checks
/// this, `serve` too, which serves a model given alone itself.
pub fn address_needs_model(embed: bool, url: bool) -> Result<(), String> {
    match (embed, url) {
        (false, true) => Err("--embed-url needs --embed".into()),
        _ => Ok(()),
    }
}

/// A model to rank by comes with where it's served, and the other way
/// about: one alone would serve searches the model can't answer.
/// `serve-many`'s rule, once, which an engine's parser checks too.
pub fn embedding_together(embed: bool, url: bool) -> Result<(), String> {
    address_needs_model(embed, url)?;
    match (embed, url) {
        (true, false) => Err("--embed needs --embed-url".into()),
        _ => Ok(()),
    }
}

/// `bridge`'s flags that go together: a team's name and whether it
/// writes go with its ID, and notes with the storage bridge they're
/// written through. Its check hook, for the machine building the line and
/// the program reading it.
pub fn bridge_flags(args: &tool::MemoryToolBridgeArgs) -> Result<(), String> {
    if args.team.is_none() && (args.team_name.is_some() || args.team_writes) {
        return Err("--team-name and --team-writes need --team".into());
    }
    if args.team.is_some() && args.team_name.is_none() {
        return Err("--team needs --team-name".into());
    }
    if args.notes_most.is_some() && args.store.is_none() {
        return Err("--notes-most needs --store".into());
    }
    Ok(())
}

#[cfg(test)]
link_test_kit::program!(unit);

#[cfg(test)]
mod tests {
    use crate::declared::{Agents, Launch};

    /// The agents file and `command`'s answer read through the generated
    /// types and write back byte for byte: their wire shape is the one
    /// engines and Link already speak, an empty environment and a false
    /// or empty field left out.
    #[test]
    fn the_agents_file_and_a_launch_read_back_as_written() {
        let file = r#"{"ada0000000aa":{"argv":["/bin/lnk-memory-tool","serve","/h"],"env":{"LNK_MEMORY_MODEL":"http://127.0.0.1:1"}},"bea0000000bb":{"argv":["/bin/lnk-memory-tool","serve","/b"]}}"#;
        let agents: Agents = serde_json::from_str(file).unwrap();
        assert_eq!(agents.0["bea0000000bb"].argv[2], "/b");
        assert!(agents.0["bea0000000bb"].env.is_empty());
        assert_eq!(serde_json::to_string(&agents).unwrap(), file);

        let launch = r#"{"allow":["search","read"],"argv":["/bin/lnk-memory-tool","serve","/h"],"model":"qwen3-embedding","models":true,"programs":["document-converter"],"shared":true}"#;
        let read: Launch = serde_json::from_str(launch).unwrap();
        assert_eq!(serde_json::to_string(&read).unwrap(), launch);
        let bare: Launch = serde_json::from_str(r#"{"argv":["/p"]}"#).unwrap();
        assert_eq!(serde_json::to_string(&bare).unwrap(), r#"{"argv":["/p"]}"#);
        assert!(serde_json::from_str::<Launch>(r#"{"argv":[],"other":1}"#).is_err());
    }
}
