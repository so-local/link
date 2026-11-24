//! Portable schedule definitions. Native source identity is separate from display data.
pub use link_interactions_contract::{ScheduleId, ScheduledOccurrence};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub id: ScheduleId,
    pub name: String,
    pub when: When,
    pub message: String,
    pub to: Destination,
    pub enabled: bool,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
}

/// When a schedule fires, and where it goes, declared in the protocol's
/// `types.toml`.
pub use crate::declared::{
    Destination, DestinationOwner, DestinationThread, When, WhenAt, WhenCron, WhenEvery,
};

/// The native job a schedule was carried from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    /// The harness it came from (`openclaw`).
    pub adapter: String,
    /// Its id in that harness's own schedules.
    pub job: String,
}

/// Every native job is carried or explicitly omitted. The revision is an
/// opaque adapter token used to refuse clearing a source that has changed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Schedules {
    pub carried: Vec<Schedule>,
    pub not_carried: Vec<NotCarried>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    /// Adapter-owned semantic inputs outside the jobs file, such as its zone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_context: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotCarried {
    pub job: String,
    pub name: String,
    pub reason: String,
}

impl Schedule {
    /// Structural validation at every importing boundary. Timing calculation
    /// and platform zone availability remain the scheduler's responsibility.
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.id.0.is_empty() && !self.name.trim().is_empty(),
            "a schedule needs an identity and name"
        );
        anyhow::ensure!(
            !self.message.trim().is_empty(),
            "a schedule needs a message"
        );
        anyhow::ensure!(
            !self.id.0.contains(char::is_control) && !self.name.contains(char::is_control),
            "schedule identity and name contain control characters"
        );
        anyhow::ensure!(self.revision > 0, "a schedule revision starts at one");
        if let Some(origin) = &self.origin {
            anyhow::ensure!(
                !origin.adapter.is_empty() && !origin.job.is_empty(),
                "a native schedule needs its source identity"
            );
        }
        match &self.when {
            When::Every(WhenEvery { seconds, .. }) => {
                anyhow::ensure!(*seconds > 0, "an interval must be positive")
            }
            When::Cron(WhenCron { cron, zone }) => anyhow::ensure!(
                cron.split_whitespace().count() == 5 && !zone.is_empty(),
                "cron needs five fields and an explicit zone"
            ),
            When::At(WhenAt { .. }) => {}
        }
        if let Destination::Owner(DestinationOwner {
            connection: Some(connection),
        }) = &self.to
        {
            anyhow::ensure!(
                !connection.is_empty()
                    && connection
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "owner destination needs a connection name"
            );
        }
        if let Destination::Thread(DestinationThread { thread }) = &self.to {
            anyhow::ensure!(
                thread
                    .split_once(':')
                    .is_some_and(|(channel, chat)| !channel.is_empty() && !chat.is_empty())
                    && !thread.contains(char::is_control),
                "a schedule destination needs a channel and chat/thread identity"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn schedule() -> Schedule {
        Schedule {
            id: ScheduleId("native:openclaw:morning".into()),
            name: "Morning briefing".into(),
            when: When::Cron(WhenCron {
                cron: "0 9 * * 1-5".into(),
                zone: "Europe/Paris".into(),
            }),
            message: "Give me today's briefing".into(),
            to: Destination::Thread(DestinationThread {
                thread: "telegram:42:topic:7".into(),
            }),
            enabled: true,
            revision: 1,
            origin: Some(Origin {
                adapter: "openclaw".into(),
                job: "morning".into(),
            }),
        }
    }
    #[test]
    fn schedules_retain_timing_destination_identity_and_omissions_on_the_wire() {
        let schedules = Schedules {
            carried: vec![schedule()],
            not_carried: vec![NotCarried {
                job: "command".into(),
                name: "Backup".into(),
                reason: "command payload cannot be a message".into(),
            }],
            source_revision: Some("revision".into()),
            source_context: None,
        };
        let state = crate::State {
            schedules: Some(schedules),
            ..Default::default()
        };
        assert_eq!(state.kinds(), [crate::Kind::Schedules]);
        let wire = serde_json::to_value(&state).unwrap();
        assert_eq!(
            wire["schedules"]["carried"][0]["when"]["zone"],
            "Europe/Paris"
        );
        assert_eq!(serde_json::from_value::<crate::State>(wire).unwrap(), state);
    }
    #[test]
    fn malformed_definitions_are_refused_instead_of_guessed() {
        let mut job = schedule();
        job.check().unwrap();
        job.when = When::Every(WhenEvery {
            seconds: 0,
            anchor: 1234,
        });
        assert!(job.check().is_err());
        job.when = When::Every(WhenEvery {
            seconds: 600,
            anchor: 1234,
        });
        job.to = Destination::Thread(DestinationThread {
            thread: "42".into(),
        });
        assert!(job.check().is_err());
        let wire =
            serde_json::json!({"kind":"cron", "cron":"* * * * *", "zone":"UTC", "typo":true});
        assert!(serde_json::from_value::<When>(wire).is_err());
    }
}
