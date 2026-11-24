//! A store snapshot retains explicit uncertainty alongside its events.

use super::*;
use link_harness_wire::evidence::export::{Gap, ReadState};

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub events: Vec<Event>,
    pub read: ReadState,
}

impl Snapshot {
    pub fn unknown(events: Vec<Event>) -> Self {
        Self {
            events,
            read: ReadState::Unknown {
                reason: "this store supplies no snapshot-completeness guarantee".into(),
            },
        }
    }

    pub(super) fn captured(events: Vec<Event>, mut gaps: Vec<Gap>) -> anyhow::Result<Self> {
        if let Some(first) = events.first()
            && first.seq != 1
        {
            gaps.push(Gap::MissingPrefix {
                first_seq: first.seq,
            });
        }
        for pair in events.windows(2) {
            if pair[0].seq.checked_add(1) != Some(pair[1].seq) {
                bail!("snapshot event {} is out of order", pair[1].seq);
            }
        }
        let read = match gaps.is_empty() {
            true => ReadState::Complete,
            false => ReadState::Incomplete { gaps },
        };
        Ok(Self { events, read })
    }
}

impl Files {
    pub(super) fn snapshot_read(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Snapshot> {
        let folder = self.folder(agent, thread, false)?;
        match (self.kept(agent, folder.as_ref())?, folder) {
            (Some(Way::Driver(_)), _) => Ok(Snapshot::unknown(self.read(agent, thread)?)),
            (Some(Way::Compact), Some(folder)) => frames::snapshot(&folder),
            (Some(Way::Log), Some(folder)) => log::snapshot(&folder),
            _ => Snapshot::captured(Vec::new(), Vec::new()),
        }
    }
}
