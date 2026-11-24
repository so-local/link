//! An agent's schedules as its spec writes them: what fires when, and
//! where it goes. Which schedule each is to its runner, its identity and
//! revision and the native job it came from, is the agent's saved
//! record's, never the spec's (`link_agent::schedules`).
use super::*;
/// A schedule the agent keeps: a message sent to it at a time, as if
/// from where it goes, as `types.toml` declares it.
pub use crate::declared::AgentSchedule as Schedule;

impl Schedule {
    /// The schedule a runner fires: this, with its identity.
    pub fn with(
        &self,
        id: link_agent_state_protocol::schedules::ScheduleId,
        revision: u64,
        origin: Option<link_agent_state_protocol::schedules::Origin>,
    ) -> link_agent_state_protocol::schedules::Schedule {
        link_agent_state_protocol::schedules::Schedule {
            id,
            name: self.name.clone(),
            when: self.when.clone(),
            message: self.message.clone(),
            to: self.to.clone(),
            enabled: self.enabled,
            revision,
            origin,
        }
    }

    /// The part of a runner's schedule its spec writes.
    pub fn of(schedule: &link_agent_state_protocol::schedules::Schedule) -> Self {
        Self {
            name: schedule.name.clone(),
            when: schedule.when.clone(),
            message: schedule.message.clone(),
            to: schedule.to.clone(),
            enabled: schedule.enabled,
        }
    }

    /// Checked as a runner's schedule is, with a stand-in identity.
    pub fn check(&self) -> anyhow::Result<()> {
        self.with(
            link_agent_state_protocol::schedules::ScheduleId("spec".into()),
            1,
            None,
        )
        .check()
    }
}

/// `text`, an agent's spec, with its schedules `schedules` and nothing
/// else changed: its comments, order and other keys kept.
pub fn with_schedules(text: &str, schedules: &[Schedule]) -> anyhow::Result<String> {
    parse(text)?;
    let mut doc = editable_document(text)?;
    let entries = Spec {
        schedules: Some(schedules.to_vec()),
        ..Default::default()
    };
    check(&entries)?;
    let mut incoming: toml_edit::DocumentMut = toml::to_string(&entries)?.parse()?;
    doc.insert(
        "schedules",
        incoming.remove("schedules").expect("serialized schedules"),
    );
    let written = doc.to_string();
    parse(&written)?;
    Ok(written)
}
