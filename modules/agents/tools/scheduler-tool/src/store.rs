//! Where the jobs are kept ([`JobStore`]): the seam between the
//! scheduler and its jobs' reading and writing. A folder of the agent's
//! ([`Stored`], `~/Link/Agents/<agent>/scheduler`) on someone's own
//! machine; the store bridge ([`Bridge`]) on a machine that runs the
//! agent only while it answers, so its jobs outlive it there and any
//! machine it's bound to next reads them; one in memory for tests
//! ([`Memory`]). What a job is, when it fires and how it moves between
//! waiting and being fired is [`crate::Store`]'s, the same whichever
//! keeps them.

use std::path::{Path, PathBuf};

use anyhow::Context;

use link_format::{Foreign, sentence};

use crate::{Caller, Job, Kind};

/// Where a job is: waiting, being fired, or its firing ending (taken
/// from being fired, so a cancel meanwhile finds it in neither). The
/// store contract's places, so a store bridge keeps them as they are.
pub use link_store_contract::store::Place;

/// What keeps an agent's jobs, and the day's count of its messages. Each
/// call is whole: two processes (the tool and the runner) never see half
/// of a job, and a job moved ([`JobStore::shift`]) is in one place or
/// the other, never both, so of two moves of one job only one is made.
pub trait JobStore: Send + Sync {
    /// Keeps `job` waiting, in place of any waiting with its ID.
    fn put(&self, job: &Job) -> anyhow::Result<()>;

    /// Job `id` in `place`, if it's there.
    fn get(&self, place: Place, id: &str) -> Option<Job>;

    /// Every job in `place`, in no order.
    fn list(&self, place: Place) -> Vec<Job>;

    /// How many jobs are in `place`. A store that can, counts them
    /// without reading them.
    fn count(&self, place: Place) -> usize {
        self.list(place).len()
    }

    /// The kinds of the jobs in `place`. A store that can, reads each
    /// job's kind only, its text never.
    fn kinds(&self, place: Place) -> Vec<Kind> {
        self.list(place).into_iter().map(|job| job.kind).collect()
    }

    /// When the soonest job waiting is due: none if none is; an error
    /// when it can't be read. A store that can, reads no job for it.
    fn next(&self) -> anyhow::Result<Option<u64>> {
        Ok(self.list(Place::Waiting).iter().map(|job| job.due).min())
    }

    /// Keeps `job` ended, with its outcome, in place of any ended with
    /// its ID.
    fn put_ended(&self, job: &Job) -> anyhow::Result<()>;

    /// Moves job `id` from `from` to `to`: whether it was in `from`.
    fn shift(&self, id: &str, from: Place, to: Place) -> bool;

    /// Removes job `id` from `place`: whether it was there.
    fn remove(&self, place: Place, id: &str) -> bool;

    /// The day's count: the day, as days since 1970 in the agent's time
    /// zone, and how many messages went on it. None kept: `(0, 0)`.
    fn day(&self) -> (u64, u32);

    /// Keeps the day's count.
    fn set_day(&self, day: u64, sent: u32) -> anyhow::Result<()>;

    /// A mark that differs once a job is put, taken or removed from
    /// waiting: what the runner, resident, watches between its looks.
    fn changed(&self) -> Option<u64>;
}

/// Jobs kept through the storage contract, on this machine or a bridge.
#[derive(Clone)]
pub struct Stored {
    store: std::sync::Arc<dyn link_store_contract::store::Store + Send>,
    agent: link_store_contract::store::Agent,
}

impl Stored {
    /// The file driver's scheduler folder: the jobs live under its
    /// `schedule/`.
    pub fn file(dir: &Path) -> anyhow::Result<Self> {
        let agent = link_store_contract::store::Agent {
            id: "scheduler".into(),
            owner: "scheduler@local".into(),
        };
        let store = link_store_contract::store::file::Folder::at(dir, &agent.id)?;
        Ok(Self {
            store: std::sync::Arc::new(store),
            agent,
        })
    }
}

/// The jobs in this process's memory, gone with it: tests, and a
/// harness that runs the scheduler in its own process.
#[derive(Default)]
pub struct Memory {
    kept: std::sync::Mutex<Kept>,
}

#[derive(Default)]
struct Kept {
    jobs: std::collections::BTreeMap<(Place, String), Job>,
    day: (u64, u32),
    /// Counts the changes to the waiting jobs.
    changes: u64,
}

impl Memory {
    fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl JobStore for Memory {
    fn put(&self, job: &Job) -> anyhow::Result<()> {
        link_machine::machine::check_id(&job.id)?;
        let mut kept = self.kept();
        kept.jobs
            .insert((Place::Waiting, job.id.clone()), job.clone());
        kept.changes += 1;
        Ok(())
    }

    fn put_ended(&self, job: &Job) -> anyhow::Result<()> {
        link_machine::machine::check_id(&job.id)?;
        self.kept()
            .jobs
            .insert((Place::Ended, job.id.clone()), job.clone());
        Ok(())
    }

    fn get(&self, place: Place, id: &str) -> Option<Job> {
        self.kept().jobs.get(&(place, id.to_string())).cloned()
    }

    fn list(&self, place: Place) -> Vec<Job> {
        let kept = self.kept();
        kept.jobs
            .iter()
            .filter(|((at, _), _)| *at == place)
            .map(|(_, job)| job.clone())
            .collect()
    }

    fn shift(&self, id: &str, from: Place, to: Place) -> bool {
        let mut kept = self.kept();
        let Some(job) = kept.jobs.remove(&(from, id.to_string())) else {
            return false;
        };
        kept.jobs.insert((to, id.to_string()), job);
        if from == Place::Waiting || to == Place::Waiting {
            kept.changes += 1;
        }
        true
    }

    fn remove(&self, place: Place, id: &str) -> bool {
        let mut kept = self.kept();
        let removed = kept.jobs.remove(&(place, id.to_string())).is_some();
        if removed && place == Place::Waiting {
            kept.changes += 1;
        }
        removed
    }

    fn day(&self) -> (u64, u32) {
        self.kept().day
    }

    fn set_day(&self, day: u64, sent: u32) -> anyhow::Result<()> {
        self.kept().day = (day, sent);
        Ok(())
    }

    fn changed(&self) -> Option<u64> {
        Some(self.kept().changes)
    }
}

/// The jobs behind the store bridge (`link_store_contract::store`): each call one
/// of its job operations, proved as the agent (`caller`) with its key
/// from the machine's keyring, read at each call, as each firing is. The
/// bridge's driver keeps them where the machine says, off it on a machine
/// that runs the agent only while it answers; a job is its JSON, kept
/// whole, its place and when it's due, so the store answers what's due
/// without reading one.
///
/// What can't be read is said on stderr and read as none: a job not
/// found, none listed, a shift not made; [`JobStore::next`], what the
/// machine is told, is an error instead.
#[derive(Clone, Debug)]
pub struct Bridge {
    pub socket: PathBuf,
    pub caller: Caller,
}

impl Bridge {
    /// The bridge at `socket`, called as `caller`.
    pub fn new(socket: &Path, caller: Caller) -> Self {
        Self {
            socket: socket.to_path_buf(),
            caller,
        }
    }
}

/// Resolves the store at each call: a bridge's binding may have changed.
trait Located: Send + Sync {
    fn bridged(
        &self,
    ) -> anyhow::Result<(
        std::sync::Arc<dyn link_store_contract::store::Store + Send>,
        link_store_contract::store::Agent,
    )>;
    fn description(&self) -> String;

    fn or_said<T>(
        &self,
        what: &str,
        call: impl FnOnce(
            &dyn link_store_contract::store::Store,
            &link_store_contract::store::Agent,
        ) -> anyhow::Result<T>,
    ) -> Option<T> {
        match self
            .bridged()
            .and_then(|(store, agent)| call(&*store, &agent))
        {
            Ok(answer) => Some(answer),
            Err(err) => {
                crate::warn_why(
                    sentence!(
                        "The store failed while {}, through {}.",
                        what,
                        Foreign::new(&self.description())
                    ),
                    &err,
                );
                None
            }
        }
    }
}

impl Located for Stored {
    fn bridged(
        &self,
    ) -> anyhow::Result<(
        std::sync::Arc<dyn link_store_contract::store::Store + Send>,
        link_store_contract::store::Agent,
    )> {
        Ok((self.store.clone(), self.agent.clone()))
    }
    fn description(&self) -> String {
        "the schedule's file driver".into()
    }
}

impl Located for Bridge {
    fn bridged(
        &self,
    ) -> anyhow::Result<(
        std::sync::Arc<dyn link_store_contract::store::Store + Send>,
        link_store_contract::store::Agent,
    )> {
        let binding = self.caller.binding()?;
        let agent = link_store_contract::store::Agent {
            id: binding.identity.agent.clone(),
            owner: binding.identity.owner.clone(),
        };
        Ok((
            std::sync::Arc::new(link_store_contract::store::Bridged {
                socket: self.socket.clone(),
                binding,
            }),
            agent,
        ))
    }
    fn description(&self) -> String {
        format!("the store bridge at {}", self.socket.display())
    }
}

/// A job as the store keeps it.
fn kept(job: &Job) -> anyhow::Result<link_store_contract::store::Job> {
    Ok(link_store_contract::store::Job {
        id: job.id.clone(),
        spec: serde_json::to_value(job)?,
        due: job.due,
    })
}

/// A job from what the store keeps: none for one that isn't a job.
fn unkept(job: link_store_contract::store::Job) -> Option<Job> {
    serde_json::from_value(job.spec).ok()
}

impl<T: Located> JobStore for T {
    fn put(&self, job: &Job) -> anyhow::Result<()> {
        link_machine::machine::check_id(&job.id)?;
        let (bridged, agent) = self.bridged()?;
        bridged
            .job_put(&agent, Place::Waiting, &kept(job)?)
            .with_context(|| format!("keeping job {} at the store bridge", job.id))
    }

    fn put_ended(&self, job: &Job) -> anyhow::Result<()> {
        link_machine::machine::check_id(&job.id)?;
        let (bridged, agent) = self.bridged()?;
        bridged
            .job_put(&agent, Place::Ended, &kept(job)?)
            .with_context(|| format!("keeping job {} ended at the store bridge", job.id))
    }

    fn get(&self, place: Place, id: &str) -> Option<Job> {
        self.or_said("reading a job", |bridged, agent| {
            bridged.job_get(agent, place, id)
        })
        .flatten()
        .and_then(unkept)
    }

    fn list(&self, place: Place) -> Vec<Job> {
        self.or_said("listing the jobs", |bridged, agent| {
            bridged.job_list(agent, place)
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(unkept)
        .collect()
    }

    /// Counted by the store, none read.
    fn count(&self, place: Place) -> usize {
        self.or_said("counting the jobs", |bridged, agent| {
            bridged.job_count(agent, place)
        })
        .map_or(0, |count| count as usize)
    }

    /// The store's answer, no job read.
    fn next(&self) -> anyhow::Result<Option<u64>> {
        let (bridged, agent) = self.bridged()?;
        bridged
            .job_next(&agent)
            .with_context(|| format!("asking {}", self.description()))
    }

    /// Whole, by the store: of two shifts of one job, one is made.
    fn shift(&self, id: &str, from: Place, to: Place) -> bool {
        self.or_said("moving a job", |bridged, agent| {
            bridged.job_shift(agent, id, from, to)
        })
        .unwrap_or(false)
    }

    fn remove(&self, place: Place, id: &str) -> bool {
        self.or_said("removing a job", |bridged, agent| {
            bridged.job_remove(agent, place, id)
        })
        .unwrap_or(false)
    }

    fn day(&self) -> (u64, u32) {
        self.or_said("reading the day's count", |bridged, agent| {
            bridged.job_day(agent)
        })
        .unwrap_or_default()
    }

    fn set_day(&self, day: u64, sent: u32) -> anyhow::Result<()> {
        let (bridged, agent) = self.bridged()?;
        bridged
            .job_set_day(&agent, day, sent)
            .context("keeping the day's count at the store bridge")
    }

    /// The store's mark: none while it can't be read.
    fn changed(&self) -> Option<u64> {
        self.or_said("reading the jobs' mark", |bridged, agent| {
            bridged.job_mark(agent)
        })
    }
}
