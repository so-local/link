//! What Link file, `lnk-file-tool`, takes: its command line ([`cli`]),
//! which its callers build from these types and the tool reads back
//! through them, and the rule its `serve` holds across flags
//! ([`team_bridge`]).

/// The calls a model makes of the tool, as its declarations render them:
/// the template its program fills ([`link_actions::tools::offer`]).
pub const TOOLS: &str = include_str!(concat!(env!("OUT_DIR"), "/tools.json"));

/// Every call the tool declares, by the name a model calls it.
pub fn calls() -> anyhow::Result<Vec<String>> {
    Ok(link_actions::tools::Template::parse(TOOLS)?.calls())
}

mod checks;
pub mod cli;

/// `serve`'s flags for teams reached through the files bridge, given
/// together or not at all: a `--team-bridge` needs `--socket`, `--as` and
/// `--account`, and each of those, or `--keyring`, needs a team to reach.
/// The check hook `team_bridge` of the tool's declarations, for both the
/// arguments a caller builds and those the tool reads.
pub fn team_bridge(
    teams: bool,
    socket: bool,
    agent: bool,
    account: bool,
    keyring: bool,
) -> Result<(), String> {
    match (
        teams,
        socket && agent && account,
        socket || agent || account || keyring,
    ) {
        (true, false, _) => Err("--team-bridge needs --socket, --as and --account".into()),
        (false, _, true) => {
            Err("--socket, --as, --account and --keyring are for a --team-bridge".into())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::cli::FileToolServeArgs;
    use link_actions::value::Validate;

    fn serve(teams: &[&str], socket: bool, agent: bool, account: bool) -> FileToolServeArgs {
        FileToolServeArgs {
            no_find: false,
            no_diffs: false,
            team_bridge: teams.iter().map(|t| t.to_string()).collect(),
            socket: socket.then(|| "/run/files.sock".into()),
            r#as: agent.then(|| "a1b2c3d4e5f6".into()),
            account: account.then(|| "acct".into()),
            keyring: None,
            command: vec!["lnk".into()],
            write: Vec::new(),
            whole: false,
            team_write: Vec::new(),
            team_read: Vec::new(),
        }
    }

    /// A team reached through the files bridge needs the bridge's socket,
    /// agent and account, and those flags need a team: as clap held them
    /// before, now the declaration's check hook, for callers and the tool
    /// alike.
    #[test]
    fn a_team_bridge_and_its_flags_come_together() {
        assert!(serve(&[], false, false, false).validate("").is_ok());
        assert!(serve(&["crew"], true, true, true).validate("").is_ok());
        let missing = serve(&["crew"], true, false, true)
            .validate("")
            .unwrap_err();
        assert!(
            missing.why.contains("needs --socket, --as and --account"),
            "{missing}"
        );
        let alone = serve(&[], true, false, false).validate("").unwrap_err();
        assert!(alone.why.contains("are for a --team-bridge"), "{alone}");
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
