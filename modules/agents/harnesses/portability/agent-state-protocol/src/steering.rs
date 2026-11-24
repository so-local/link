//! Steering and queueing: what a prompt sent while the agent is still
//! answering does. A client says it in the prompt's `_meta.asp.busy`; the
//! agent says which it takes in its capability's [`Steering`], and
//! refuses a prompt asking for another, never silently doing one it
//! wasn't asked for.

use serde::{Deserialize, Serialize};

use crate::acp::schema::v1::Meta;

/// What a prompt sent while a turn runs does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Busy {
    /// Joins the answer under way, at the agent's next step.
    Steer,
    /// Waits for the turn to end, then gets one of its own.
    Queue,
    /// Cancels the turn, and starts the next.
    Interrupt,
}

/// Which [`Busy`] an agent takes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Steering {
    pub busy: Vec<Busy>,
}

/// What a prompt's `_meta` asks of it while a turn runs: none when it
/// says nothing; an error for an `asp.busy` that isn't one.
pub fn asked(meta: Option<&Meta>) -> Result<Option<Busy>, String> {
    let Some(busy) = meta.and_then(|m| m.get(crate::META)?.get("busy")) else {
        return Ok(None);
    };
    serde_json::from_value(busy.clone())
        .map(Some)
        .map_err(|_| format!("{busy} isn't steer, queue or interrupt"))
}

/// A prompt's `_meta` asking `busy`, beside whatever else it holds.
pub fn asking(busy: Busy, mut meta: Meta) -> Meta {
    let asp = meta
        .entry(crate::META)
        .or_insert_with(|| serde_json::Value::Object(Meta::new()));
    if let Some(asp) = asp.as_object_mut() {
        asp.insert(
            "busy".into(),
            serde_json::to_value(busy).unwrap_or_default(),
        );
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Advertised;
    use crate::acp::schema::v1::AgentCapabilities;

    #[test]
    fn a_prompt_says_what_it_does_while_a_turn_runs() {
        let meta = asking(Busy::Interrupt, Meta::new());
        assert_eq!(
            serde_json::Value::Object(meta.clone()),
            serde_json::json!({"asp": {"busy": "interrupt"}})
        );
        assert_eq!(asked(Some(&meta)), Ok(Some(Busy::Interrupt)));
        assert_eq!(asked(None), Ok(None));
        let odd = serde_json::json!({"asp": {"busy": "teleport"}});
        assert!(asked(odd.as_object()).is_err());
    }

    #[test]
    fn the_agent_says_which_it_takes_beside_its_state() {
        let advertised = Advertised {
            steering: Some(Steering {
                busy: vec![Busy::Queue, Busy::Interrupt],
            }),
            ..Advertised::default()
        };
        let wire = serde_json::to_value(advertised.on(AgentCapabilities::new())).unwrap();
        assert_eq!(
            wire["_meta"]["asp"],
            serde_json::json!({"steering": {"busy": ["queue", "interrupt"]}})
        );
        let back: AgentCapabilities = serde_json::from_value(wire).unwrap();
        assert_eq!(Advertised::of(&back), advertised);
        assert_eq!(crate::AgentState::of(&back), None);
    }
}
