//! Live unit selection through the actual measure command.
use super::*;
#[test]
fn agent_and_pid_views_select_only_the_requested_live_unit() {
    let w = World::new();
    if !w.trusted_home() {
        return;
    }
    w.agents(&[w.agent("a1", true)], false);
    assert!(measure(&w, &["set", "--processes"]).status.success());
    let all = measure(&w, &["--json"]);
    assert!(all.status.success(), "{all:?}");
    let all: Value = serde_json::from_slice(&all.stdout).unwrap();
    assert!(
        all["units"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["kind"] == "machine")
    );
    let agent = measure(&w, &["--agent", "a1", "--json"]);
    assert!(agent.status.success(), "{agent:?}");
    let agent: Value = serde_json::from_slice(&agent.stdout).unwrap();
    assert_eq!(agent["units"][0]["kind"], "agent");
    assert_eq!(agent["units"][0]["name"], "a1");
    assert!(
        !agent["units"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["kind"] == "machine")
    );
    let pid = std::process::id().to_string();
    let one = measure(&w, &["--agent", "a1", "--pid", &pid, "--json"]);
    assert!(one.status.success(), "{one:?}");
    let one: Value = serde_json::from_slice(&one.stdout).unwrap();
    let units = one["units"].as_array().unwrap();
    assert_eq!(units.len(), 1, "{one}");
    assert_eq!(units[0]["parent"], "a1");
    assert!(
        units[0]["id"]
            .as_str()
            .unwrap()
            .ends_with(&format!("/{pid}"))
    );
    for args in [
        vec!["--agent", "qa-no-agent", "--json"],
        vec!["--agent", "a1", "--pid", "0", "--json"],
        vec!["--pid", &pid, "--json"],
    ] {
        let refused = measure(&w, &args);
        assert_eq!(refused.status.code(), Some(2), "{refused:?}");
        assert!(refused.stdout.is_empty());
    }
}
