//! The generated hosting shapes write what the hand-written ones they
//! replaced wrote. `was` keeps those shapes as they were, here alone, so
//! each case writes one value through both and compares the JSON. The
//! generator orders a record's keys by name, which JSON readers ignore,
//! so values are compared, never bytes. Two cases differ, each on
//! purpose: a team chat with no member left out where `null` was
//! written, and an empty `have` left out where `{}` was.

use super::*;
use serde_json::{Value, json};

/// The shapes as they were written by hand, before `types.toml` declared
/// them.
mod was {
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize)]
    pub struct Request {
        pub account: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub create: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub connect: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub team: Option<Action>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub owner: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub agent: Option<HostedAgent>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub size: Option<u64>,
        #[serde(rename = "move", default, skip_serializing_if = "Option::is_none")]
        pub move_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub phase: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub have: Option<BTreeMap<String, String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub seal_to: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub thread: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub text: Option<String>,
    }
    use serde_json::Value;

    #[derive(Serialize, Deserialize)]
    pub struct Reply {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub agent: Option<HostedAgent>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        pub managed_models: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub seal_to: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub team: Option<TeamReply>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub size: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub runs_agents: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub agents: Option<Vec<HostedAgent>>,
        #[serde(rename = "move", default, skip_serializing_if = "Option::is_none")]
        pub move_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub have: Option<BTreeMap<String, String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub refused: Option<Refused>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub answers: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub machine: Option<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        pub team_held: BTreeMap<String, Vec<Held>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub held: Vec<Held>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub at: Option<u64>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct HostedAgent {
        pub id: String,
        pub name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub owner: Option<String>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub state: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Refused {
        pub why: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub retry_at: Option<u64>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Held {
        pub connection: String,
        pub chat: String,
        pub from: String,
        pub text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub generation: Option<String>,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
    pub enum Action {
        Chat {
            team: String,
            member: Option<String>,
        },
        List,
        Folder {
            team: String,
        },
        Backup {
            team: String,
        },
        Restore {
            team: String,
        },
        BeginIn {
            file: String,
            agents: Vec<HostedAgent>,
            size: u64,
        },
        BeginOut {
            team: String,
        },
        Get {
            team: String,
        },
        Put {
            team: String,
            file: String,
            version: u64,
        },
        Apply {
            file: String,
            version: u64,
        },
        Start {
            team: String,
        },
        Stop {
            team: String,
        },
        Delete {
            team: String,
        },
        Invites {
            agent: String,
        },
        Join {
            owner: String,
            team: String,
            agent: String,
        },
        Leave {
            owner: String,
            team: String,
            agent: String,
        },
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct TeamReply {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub file: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub movement: Option<Movement>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub version: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub at: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub teams: Option<Vec<Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub invites: Option<Vec<Value>>,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Movement {
        pub id: String,
        pub file: String,
        pub members: Vec<MovingMember>,
        pub folder: String,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct MovingMember {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        pub managed_models: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub seal_to: Option<String>,
        pub id: String,
        pub name: String,
        pub movement: String,
    }
}

/// `value` read as the old shape `Old` and as the new `New`, each
/// written again: the same JSON both ways.
fn same<Old, New>(value: Value)
where
    Old: Serialize + serde::de::DeserializeOwned,
    New: Serialize + serde::de::DeserializeOwned,
{
    let old: Old = serde_json::from_value(value.clone()).expect("the old shape reads it");
    let new: New = serde_json::from_value(value.clone()).expect("the new shape reads it");
    let old = serde_json::to_value(old).unwrap();
    assert_eq!(old, value, "the old shape writes it as given");
    assert_eq!(
        serde_json::to_value(new).unwrap(),
        old,
        "the new shape writes what the old did"
    );
}

fn seal_to() -> String {
    serde_json::to_value(link_secretive::Recipient::new().seal_to())
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

fn account() -> Value {
    serde_json::to_value(Account {
        name: "work".into(),
        kind: "link".into(),
        place: "us".into(),
        ..Account::default()
    })
    .unwrap()
}

fn agent() -> Value {
    json!({"id": "k3m4n5p6q7r8", "name": "main", "owner": "acct-1", "state": "running"})
}

#[test]
fn every_request_field_writes_as_it_did() {
    same::<was::Request, Request>(json!({
        "account": account(),
        "create": {"name": "main"},
        "connect": {"connection": "slack", "token": "xoxb-1", "user": "U1"},
        "team": {"action": "get", "team": "research"},
        "owner": "acct-1",
        "agent": agent(),
        "size": 4096,
        "move": "m1",
        "phase": "switch",
        "have": {"files/a": "00"},
        "seal_to": seal_to(),
        "thread": "link:main",
        "text": "hello",
    }));
    same::<was::Request, Request>(json!({"account": account()}));
}

#[test]
fn every_reply_field_writes_as_it_did() {
    let held = json!({"connection": "slack", "chat": "C1", "from": "U1", "text": "hi", "generation": "m1"});
    same::<was::Reply, Reply>(json!({
        "message": "done",
        "agent": agent(),
        "managed_models": true,
        "seal_to": seal_to(),
        "team": {"version": 3, "at": 9},
        "size": 4096,
        "runs_agents": true,
        "agents": [agent(), {"id": "a1", "name": "work"}],
        "move": "m1",
        "have": {"files/a": "00"},
        "refused": {"why": "busy", "retry_at": 60},
        "answers": true,
        "machine": "mac",
        "team_held": {"a1": [held.clone()]},
        "held": [held],
        "at": 9,
    }));
    same::<was::Reply, Reply>(json!({}));
    same::<was::Reply, Reply>(
        json!({"agents": [], "runs_agents": false, "refused": {"why": "no"}}),
    );
}

#[test]
fn every_team_action_writes_as_it_did() {
    for action in [
        json!({"action": "chat", "team": "research", "member": "scout"}),
        json!({"action": "list"}),
        json!({"action": "folder", "team": "research"}),
        json!({"action": "backup", "team": "research"}),
        json!({"action": "restore", "team": "research"}),
        json!({"action": "begin-in", "file": "name = \"research\"", "agents": [agent()], "size": 1}),
        json!({"action": "begin-out", "team": "research"}),
        json!({"action": "get", "team": "research"}),
        json!({"action": "put", "team": "research", "file": "", "version": 2}),
        json!({"action": "apply", "file": "", "version": 0}),
        json!({"action": "start", "team": "research"}),
        json!({"action": "stop", "team": "research"}),
        json!({"action": "delete", "team": "research"}),
        json!({"action": "invites", "agent": "a1"}),
        json!({"action": "join", "owner": "acct-1", "team": "t1", "agent": "a1"}),
        json!({"action": "leave", "owner": "acct-1", "team": "t1", "agent": "a1"}),
    ] {
        same::<was::Action, teams::Action>(action);
    }
}

#[test]
fn every_team_reply_writes_as_it_did() {
    same::<was::TeamReply, teams::Reply>(json!({
        "file": {"file": "name = \"research\"", "version": 2, "joined": ["service:k3m4n5p6q7r8"]},
        "movement": {"id": "g1", "file": "", "folder": "f1", "members": [
            {"managed_models": true, "seal_to": seal_to(), "id": "a1", "name": "scout", "movement": "m1"},
            {"id": "a2", "name": "writer", "movement": "m2"},
        ]},
        "version": 2,
        "at": 9,
        "teams": [{"id": "t1", "name": "research", "members": ["a1"], "owner": "acct-1",
            "relay": "https://relay.example", "files": "https://files.example/t1"}],
        "invites": [{"owner": "acct-1", "team": "t1", "name": "research", "agent": "a1",
            "purpose": "Finds sources", "write": ["drafts"], "read": [], "joined": false}],
    }));
}

/// The two differences, each where the generator leaves out what's
/// empty: both sides read either form.
#[test]
fn an_unset_member_and_an_empty_have_are_left_out() {
    let chat = json!({"action": "chat", "team": "research", "member": null});
    let old: was::Action = serde_json::from_value(chat.clone()).unwrap();
    assert_eq!(serde_json::to_value(old).unwrap(), chat);
    let new: teams::Action = serde_json::from_value(chat).unwrap();
    assert_eq!(
        serde_json::to_value(new).unwrap(),
        json!({"action": "chat", "team": "research"})
    );

    let have = json!({"have": {}});
    let old: was::Reply = serde_json::from_value(have.clone()).unwrap();
    assert_eq!(serde_json::to_value(old).unwrap(), have);
    let new: Reply = serde_json::from_value(have).unwrap();
    assert!(new.have.is_empty());
    assert_eq!(serde_json::to_value(new).unwrap(), json!({}));
}

/// A key nothing declares was read and dropped; now it's refused.
#[test]
fn an_undeclared_key_is_refused() {
    let odd = json!({"account": account(), "colour": "blue"});
    assert!(serde_json::from_value::<was::Request>(odd.clone()).is_ok());
    assert!(serde_json::from_value::<Request>(odd).is_err());
}
