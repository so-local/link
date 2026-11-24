//! The owner's state export uses the same capture as a harness switch.

use link_agent::{adapter::Adapter, settings::Settings};
use link_format::{Done, Name, Out, Report, WithinExt, sentence};

pub(crate) async fn run(out: &mut Out, agent: &Settings) -> Result<Done, Report> {
    let name = agent.harness();
    let adapter = Adapter::find(name)?;
    let state = crate::switching::exported(out, agent, (name, &adapter))
        .await?
        .within("this harness cannot export state while stopped; start the agent first")?;
    link_actions::answer::reply::<link_agent_spec::declared::State>(out, &state)?;
    Ok(Done::success(sentence!(
        "Captured state for {}.",
        Name::new("agent", &agent.name)
    )))
}

#[cfg(test)]
mod tests {
    /// A state export holding a conversation's retained evidence is its
    /// declared output, the evidence's agent named by whose it is and its
    /// ID.
    #[test]
    fn a_state_with_session_evidence_is_its_declared_output() {
        let state: link_harness_contract::asp::State = serde_json::from_value(serde_json::json!({
            "sessions": [{
                "sessionId": "s1",
                "updates": [],
                "evidence": {
                    "version": 1,
                    "conversation": {"agent": {"owner": "owner", "id": "a1"}, "thread": "link:main"},
                    "events": [],
                    "imported": [],
                    "read": {"kind": "complete"}
                }
            }]
        }))
        .unwrap();
        let declared: link_agent_spec::declared::State =
            link_actions::answer::output(&state).unwrap();
        assert_eq!(
            serde_json::to_value(&declared).unwrap(),
            serde_json::to_value(&state).unwrap()
        );
    }
}
