//! Agent state's three methods, extension methods of ACP until ACP
//! takes them: each a request a client sends an agent that advertises
//! [`crate::AgentState`], typed for the SDK's connections.

use std::collections::BTreeMap;

use crate::acp::schema::v1::SessionId;
use crate::acp::{JsonRpcRequest, JsonRpcResponse};
use crate::forms::{Kind, State};
use serde::{Deserialize, Serialize};

/// `_asp/state/describe`: what state the agent holds now.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_asp/state/describe", response = Described)]
pub struct Describe {}

/// How many of each kind it holds: sessions, skills, notes, servers,
/// settings; one for instructions it has.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct Described {
    pub kinds: BTreeMap<Kind, usize>,
}

/// `_asp/state/export`: the kinds asked for, in portable form, asked
/// with no turn running.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_asp/state/export", response = Exported)]
pub struct Export {
    pub kinds: Vec<Kind>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub state: State,
}

/// `_asp/state/import`: state taken in, answered kind by kind.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_asp/state/import", response = Imported)]
pub struct Import {
    pub state: State,
}

/// What an import took and what it couldn't, and each session's new
/// id by its old, so a client that keeps a session per chat follows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    pub taken: Vec<Kind>,
    #[serde(default)]
    pub not_taken: Vec<NotTaken>,
    #[serde(default)]
    pub sessions: std::collections::HashMap<SessionId, SessionId>,
}

/// A kind an import couldn't take, and why, in words for the owner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NotTaken {
    pub kind: Kind,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::JsonRpcMessage;

    /// The RFD's own export and import, on the wire.
    #[test]
    fn the_methods_read_as_the_rfd_writes_them() {
        let export = Export {
            kinds: vec![Kind::Sessions, Kind::Memory],
        };
        assert_eq!(export.method(), "_asp/state/export");
        assert_eq!(Describe {}.method(), "_asp/state/describe");
        assert_eq!(Import::default().method(), "_asp/state/import");
        let params = serde_json::to_value(&export).unwrap();
        assert_eq!(params, serde_json::json!({"kinds": ["sessions", "memory"]}));

        let rfd = serde_json::json!({
            "taken": ["sessions", "instructions"],
            "notTaken": [{"kind": "memory", "reason": "this agent keeps no memory"}],
            "sessions": {"sess_789xyz": "sess_a1b2c3"}
        });
        let imported: Imported = serde_json::from_value(rfd.clone()).unwrap();
        assert_eq!(imported.taken, [Kind::Sessions, Kind::Instructions]);
        assert_eq!(imported.not_taken[0].kind, Kind::Memory);
        assert_eq!(
            imported.sessions[&SessionId::new("sess_789xyz")],
            SessionId::new("sess_a1b2c3")
        );
        assert_eq!(serde_json::to_value(&imported).unwrap(), rfd);
    }
}
