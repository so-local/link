//! ASP, the Agent State Protocol: an agent's state carried from one
//! harness to another, on ACP, the session protocol every client and
//! harness of Link speaks ([`acp`], the protocol's own SDK).
//!
//! Two layers: agent state, an [`AgentState`] capability an agent
//! advertises under its `initialize` answer's `_meta`, and three
//! extension methods ([`methods`]) a client asks it: what state it
//! holds, that state in portable form ([`forms`]), and that state taken
//! in, answered kind by kind; and [`steering`], what a prompt sent while
//! the agent answers does.

pub use agent_client_protocol as acp;

/// The protocol's shapes, generated from its declarations.
pub mod declared {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}
pub mod forms;
pub mod methods;
pub mod schedules;
pub mod steering;

use acp::schema::v1::AgentCapabilities;
use serde::{Deserialize, Serialize};

pub use forms::{Kind, State};
pub use steering::{Busy, Steering};

/// The key of `_meta` that ASP's capability is under, in an agent's
/// capabilities: `{"_meta": {"asp": {"agentState": {...}}}}`.
pub const META: &str = "asp";

/// The kinds of state an agent gives and takes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentState {
    /// What `_asp/state/export` gives.
    #[serde(default)]
    pub export: Vec<Kind>,
    /// What `_asp/state/import` takes.
    #[serde(default)]
    pub import: Vec<Kind>,
}

impl AgentState {
    /// Whether it gives and takes nothing.
    pub fn is_empty(&self) -> bool {
        self.export.is_empty() && self.import.is_empty()
    }
}

/// What an agent advertises of ASP, each layer it speaks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Advertised {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_state: Option<AgentState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steering: Option<Steering>,
}

impl Advertised {
    /// `capabilities`, advertising it under `_meta`'s [`META`], beside
    /// whatever else its `_meta` holds.
    pub fn on(&self, mut capabilities: AgentCapabilities) -> AgentCapabilities {
        let mut meta = capabilities.meta.take().unwrap_or_default();
        meta.insert(META.into(), serde_json::to_value(self).unwrap_or_default());
        capabilities.meta = Some(meta);
        capabilities
    }

    /// What an agent's `capabilities` advertise: nothing from an agent
    /// that speaks no ASP, or advertises it in a shape this isn't.
    pub fn of(capabilities: &AgentCapabilities) -> Self {
        capabilities
            .meta
            .as_ref()
            .and_then(|meta| serde_json::from_value(meta.get(META)?.clone()).ok())
            .unwrap_or_default()
    }
}

impl AgentState {
    /// `capabilities`, advertising it: under `_meta`'s [`META`], beside
    /// whatever else its `_meta` holds.
    pub fn advertised(&self, capabilities: AgentCapabilities) -> AgentCapabilities {
        Advertised {
            agent_state: Some(self.clone()),
            ..Advertised::default()
        }
        .on(capabilities)
    }

    /// What an agent's `capabilities` advertise: none from an agent
    /// that speaks no ASP, or advertises it in a shape this isn't.
    pub fn of(capabilities: &AgentCapabilities) -> Option<Self> {
        Advertised::of(capabilities).agent_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RFD's capability, as an agent's `initialize` answer carries it.
    #[test]
    fn the_capability_is_under_meta_beside_what_else_is_there() {
        let state = AgentState {
            export: vec![Kind::Sessions, Kind::Memory],
            import: vec![Kind::Sessions, Kind::Tools],
        };
        let mut other = acp::schema::v1::Meta::new();
        other.insert("vendor".into(), serde_json::json!(1));
        let capabilities = state.advertised(AgentCapabilities::new().meta(other));
        let wire = serde_json::to_value(&capabilities).unwrap();
        assert_eq!(
            wire["_meta"]["asp"]["agentState"],
            serde_json::json!({"export": ["sessions", "memory"], "import": ["sessions", "tools"]})
        );
        assert_eq!(wire["_meta"]["vendor"], 1);
        let back: AgentCapabilities = serde_json::from_value(wire).unwrap();
        assert_eq!(AgentState::of(&back), Some(state));
        assert_eq!(AgentState::of(&AgentCapabilities::new()), None);
        let odd = AgentCapabilities::new().meta(
            serde_json::json!({"asp": "no"})
                .as_object()
                .cloned()
                .unwrap(),
        );
        assert_eq!(AgentState::of(&odd), None);
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
