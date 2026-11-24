//! What a start that answered suggests next.

use link_format::{Done, cmd};

/// What a start that answered suggests: talking to the agent first, then
/// where else it can be reached and what it can do beyond talking, each
/// while it hasn't that yet.
pub(super) fn running_next(mut done: Done, connected: bool, has_run: bool, has_mcp: bool) -> Done {
    done = done.next(cmd!("lnk agent chat"), "talk to it here");
    if !connected {
        done = done.next(
            cmd!("lnk agent connect telegram"),
            "text it from your phone (or slack, discord, signal)",
        );
    }
    if !has_run {
        done = done.next(cmd!("lnk agent tool add run"), "let it run programs");
    }
    if !has_mcp {
        done = done.next(
            cmd!("lnk agent tool add mcp"),
            "let it read Link's docs and run Link commands",
        );
    }
    done.next(cmd!("lnk agent tool list"), "see more tools")
        .next(
            cmd!("lnk agent harness set <harness>"),
            "use another harness",
        )
        .next(cmd!("lnk agent model set <model>"), "use another model")
        .next(cmd!("lnk agent stop"), "stop it")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A start leads to a chat, then to what the agent doesn't have yet,
    /// each declared in `agent start`'s Nexts.
    #[test]
    fn a_running_agent_is_talked_to_first() {
        let texts =
            |done: Done| -> Vec<String> { done.next.0.iter().map(|s| s.command.text()).collect() };
        let fresh = texts(running_next(Done::default(), false, false, false));
        assert_eq!(
            fresh[..4],
            [
                "lnk agent chat",
                "lnk agent connect telegram",
                "lnk agent tool add run",
                "lnk agent tool add mcp"
            ]
        );
        let set_up = texts(running_next(Done::default(), true, true, true));
        assert_eq!(set_up[0], "lnk agent chat");
        assert!(
            set_up.iter().all(|t| !t.contains("connect")
                && !t.ends_with("add run")
                && !t.ends_with("add mcp"))
        );
        let actions: toml::Value = crate::navigation::navigation();
        assert_eq!(
            actions["agent start"]["next"][1].as_str(),
            Some("lnk agent chat <arg>")
        );
        // Each action that starts the agent ends on these.
        for action in [
            "agent start",
            "agent create",
            "agent restart",
            "agent harness set",
            "agent move",
        ] {
            let declared: Vec<String> = actions[action]["next"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t.as_str().unwrap().to_string())
                .collect();
            let typed = link_format::Command::fill(&format!("lnk {action}"), vec![]);
            for given in running_next(Done::default(), false, false, false).next.0 {
                assert!(
                    link_format::nexts::declares(&declared, &given.command, &typed),
                    "{action} doesn't declare {}",
                    given.command.text()
                );
            }
        }
    }
}
