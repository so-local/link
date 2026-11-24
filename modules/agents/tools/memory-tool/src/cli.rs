//! `lnk-memory-tool`'s command line and its outputs, generated from the
//! module's declarations (`actions.toml` and `types.toml`). Link runs
//! every command, never a person: `command` prints the `serve` line Link
//! then serves an agent with, which [`serve_line`] reads back.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk-memory-tool`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

/// The `serve` line serving `args`, after the program: what `command`
/// prints for Link, from the same declared arguments the parser reads.
pub fn serve_argv(args: &MemoryToolServeArgs) -> Vec<String> {
    let value = serde_json::to_value(args).expect("runtime arguments are JSON");
    link_actions::cli::argv(&["serve"], ARGS_MEMORYTOOLSERVE, &value)
}

/// A `serve` line, its program first, read back.
pub fn serve_line(
    argv: &[String],
    env: &dyn Fn(&str) -> Option<String>,
) -> anyhow::Result<MemoryToolServeArgs> {
    use clap::{CommandFactory, FromArgMatches};
    let matches = link_actions::cli::under(Cli::command(), env).try_get_matches_from(argv)?;
    match Cli::from_arg_matches(&matches)?.command {
        Command::MemoryToolServe(serve) => Ok(serve),
        _ => anyhow::bail!("{argv:?} isn't a serve line"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use clap::Parser;
    use link_memory_tool_contract::declared::{
        LaunchShowArgs, MemoryInstallArgs, MemoryServeManyArgs,
    };
    use std::path::PathBuf;

    /// `argv`, a line the contract's generated arguments wrote, read by
    /// this program's parser.
    fn read(argv: Vec<String>) -> Command {
        let line = std::iter::once("lnk-memory-tool".to_string()).chain(argv);
        Cli::try_parse_from(line).expect("the line parses").command
    }

    /// Each protocol line Link builds from the contract reads back here as
    /// the values it was built from: one declaration, both sides.
    #[test]
    fn a_line_the_contract_builds_reads_back_as_it_was_built() {
        let built = LaunchShowArgs {
            folder: PathBuf::from("/h/home"),
            state: PathBuf::from("/h/state"),
            agent: Some(PathBuf::from("/h/agent")),
        };
        let Command::LaunchShow(read_back) = read(built.argv()) else {
            panic!("not command");
        };
        assert_eq!(
            (read_back.folder, read_back.state, read_back.agent),
            (built.folder, built.state, built.agent)
        );

        let built = MemoryServeManyArgs {
            socket: PathBuf::from("memory.sock"),
            agents: Some(PathBuf::from("/run/agents.json")),
            bridges: Some(PathBuf::from("/run/bridges")),
            cache: Some(PathBuf::from("/cache")),
            embed: Some("qwen3-embedding".into()),
            embed_url: Some("http://127.0.0.1:1/v1".into()),
            files_bridge: Some(PathBuf::from("/run/files.sock")),
            peer: Some("units".into()),
            socket_group: Some("link".into()),
            stay: true,
            storage_bridge: Some(PathBuf::from("/run/storage.sock")),
        };
        let line = built.argv();
        assert_eq!(line[..3], ["serve-many", "memory.sock", "/run/agents.json"]);
        let Command::MemoryToolServeMany(read_back) = read(line) else {
            panic!("not serve-many");
        };
        assert_eq!(
            serde_json::to_value(&read_back).unwrap(),
            serde_json::to_value(&built).unwrap()
        );

        assert_eq!(MemoryInstallArgs {}.argv(), ["install"]);
        assert!(matches!(
            read(MemoryInstallArgs {}.argv()),
            Command::MemoryToolInstall(_)
        ));
    }

    /// A machine's `bridge` line, built from the contract's arguments,
    /// reads back here as it was built; one whose team's flags don't go
    /// together is refused by the contract's rule.
    #[test]
    fn a_bridge_line_reads_back_and_its_flags_go_together() {
        use link_memory_tool_contract::tool::MemoryToolBridgeArgs;
        let built = MemoryToolBridgeArgs {
            account: "me@box".into(),
            r#as: "agent0000001".into(),
            cache: PathBuf::from("/run/tools/agent0000001/memory/index"),
            keyring: Some(PathBuf::from("/run/keys")),
            kept: vec!["conversations=postgres".into(), "memory=postgres".into()],
            notes_most: Some(4096),
            once: true,
            socket: PathBuf::from("/run/files.sock"),
            store: Some(PathBuf::from("/run/store.sock")),
            team: Some("team00000001".into()),
            team_name: Some("crew".into()),
            team_writes: true,
        };
        let Command::MemoryToolBridge(read_back) = read(built.argv()) else {
            panic!("not bridge");
        };
        assert_eq!(
            serde_json::to_value(&read_back).unwrap(),
            serde_json::to_value(&built).unwrap()
        );
        let nameless = MemoryToolBridgeArgs {
            team_name: None,
            ..built.clone()
        };
        assert_eq!(
            link_memory_tool_contract::bridge_flags(&nameless).unwrap_err(),
            "--team needs --team-name"
        );
        let line = std::iter::once("lnk-memory-tool".to_string()).chain(nameless.argv());
        assert!(
            Cli::try_parse_from(line).is_err(),
            "the program refuses it too"
        );
    }

    /// An agent's serve line, read in serve-many's one process, takes
    /// its model's address from that agent's own environment: set, empty
    /// (served for it, by no runtime here) or unset (it serves its own).
    #[test]
    fn a_serve_line_reads_its_model_from_its_agents_environment() {
        let line: Vec<String> = ["lnk-memory-tool", "serve", "/h", "--embed", "m"]
            .map(String::from)
            .to_vec();
        let url = |value: Option<&str>| {
            let env = |key: &str| {
                (key == link_memory_tool_contract::MODEL_ENV)
                    .then(|| value.map(String::from))
                    .flatten()
            };
            super::serve_line(&line, &env).unwrap().embed_url
        };
        assert_eq!(
            url(Some("http://127.0.0.1:1")).as_deref(),
            Some("http://127.0.0.1:1")
        );
        assert_eq!(url(Some("")).as_deref(), Some(""));
        assert_eq!(url(None), None);
    }

    /// `serve` refuses a model's address with no model, by its declared
    /// check, and takes a model alone, which it serves itself.
    #[test]
    fn a_serve_line_refuses_an_address_with_no_model() {
        let none = |_: &str| None;
        let line = |words: &[&str]| -> Vec<String> {
            ["lnk-memory-tool", "serve", "/h"]
                .iter()
                .chain(words)
                .map(|w| w.to_string())
                .collect()
        };
        let refused =
            super::serve_line(&line(&["--embed-url", "http://127.0.0.1:1"]), &none).unwrap_err();
        assert!(
            format!("{refused:#}").contains("--embed-url needs --embed"),
            "{refused:#}"
        );
        let alone = super::serve_line(&line(&["--embed", "m"]), &none).unwrap();
        assert_eq!(alone.embed.as_deref(), Some("m"));
    }
}
