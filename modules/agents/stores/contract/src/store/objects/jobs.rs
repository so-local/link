//! The agent's jobs as objects, under `schedule/`: a job, a shift's claim
//! on one until it's where it goes, and the mark moved on with what waits.

use super::*;

/// A job claimed by a shift, in the place it leaves, until it's in the
/// place it goes to. Its shift finishes it; a reader finishes only one
/// whose shift stopped there ([`CLAIM_LAPSE`] old, and still there as it
/// was read), so a reader that read a claim before its shift finished
/// never writes the job back.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Moving {
    pub(super) moving_to: Place,
    pub(super) job: Job,
    /// When it was claimed, in seconds since 1970.
    pub(super) claimed: u64,
}

/// How long a shift's claim stands before a reader takes its shift as
/// stopped and finishes it: far past any call's deadline, so a shift
/// still under way always finishes itself.
pub(super) const CLAIM_LAPSE: u64 = 120;

/// What a job's object holds: the job, or a shift of it under way.
pub(super) enum JobHeld {
    Job(Job),
    Moving(Moving),
}

pub(super) fn held_job(bytes: &[u8]) -> anyhow::Result<JobHeld> {
    let value: Value = serde_json::from_slice(bytes).context("reading a job")?;
    Ok(match value.get("moving_to") {
        Some(_) => JobHeld::Moving(serde_json::from_value(value)?),
        None => JobHeld::Job(serde_json::from_value(value)?),
    })
}

/// Job `job` as a shift places it where it goes: with the claim it was
/// moved by (its object's name, and version), which whatever next
/// removes, shifts or replaces it removes first ([`Objects::finish_claim`]),
/// so a claim is left only while its job is still where it was placed.
pub(super) fn placed(job: &Job, claim: &str, version: &Version) -> anyhow::Result<Vec<u8>> {
    let mut value = serde_json::to_value(job)?;
    value["claim"] = serde_json::json!({"name": claim, "version": version});
    Ok(serde_json::to_vec(&value)?)
}

impl<S: Store> Objects<S> {
    pub(super) fn jobs_in(&self, place: Place) -> String {
        format!("schedule/{}/", place_name(place))
    }

    pub(super) fn job(&self, place: Place, id: &str) -> String {
        format!("{}{}", self.jobs_in(place), escape(id))
    }

    /// Job `id` in `place`, and its object's version: none when it isn't
    /// there, or a shift is taking it elsewhere (finished now).
    pub(super) fn job_at(&self, place: Place, id: &str) -> anyhow::Result<Option<(Job, Version)>> {
        let name = self.job(place, id);
        match self.store.get(&name)? {
            None => Ok(None),
            Some((bytes, version)) => match held_job(&bytes)? {
                JobHeld::Job(job) => Ok(Some((job, version))),
                JobHeld::Moving(_) => {
                    self.settle_stopped(&name, &bytes, &version)?;
                    Ok(None)
                }
            },
        }
    }

    pub(super) fn jobs(&self, place: Place) -> anyhow::Result<Vec<Job>> {
        let mut jobs = Vec::new();
        for object in self.store.list(&self.jobs_in(place))? {
            let Some((bytes, version)) = self.store.get(&object.path)? else {
                continue;
            };
            match held_job(&bytes)? {
                JobHeld::Job(job) => jobs.push(job),
                JobHeld::Moving(_) => self.settle_stopped(&object.path, &bytes, &version)?,
            }
        }
        Ok(jobs)
    }

    /// The claim job object `bytes` was placed by ([`placed`]) removed, if
    /// it's still the one the job names: its shift is done with.
    pub(super) fn finish_claim(&self, bytes: &[u8]) -> anyhow::Result<()> {
        let value: Value = serde_json::from_slice(bytes).context("reading a job")?;
        let (Some(name), Some(version)) = (
            value["claim"]["name"].as_str(),
            value["claim"]["version"].as_str(),
        ) else {
            return Ok(());
        };
        anyhow::ensure!(
            name.starts_with("schedule/"),
            "a job's claim names no job: {}",
            link_format::clean(name)
        );
        match self
            .store
            .delete(name, &Expect::Version(version.to_string()))
        {
            Err(err) if is_conflict(&err) => Ok(()),
            other => other,
        }
    }

    /// Finishes the shift claimed in object `name` (`bytes`, at
    /// `version`), as the shift that claimed it: the job written where it
    /// goes, then the claim removed if it's still the one written.
    pub(super) fn settle(&self, name: &str, bytes: &[u8], version: Version) -> anyhow::Result<()> {
        let JobHeld::Moving(moving) = held_job(bytes)? else {
            return Ok(());
        };
        self.store.put(
            &self.job(moving.moving_to, &moving.job.id),
            &placed(&moving.job, name, &version)?,
            &Expect::Any,
        )?;
        match self.store.delete(name, &Expect::Version(version)) {
            Err(err) if is_conflict(&err) => {}
            other => other?,
        }
        Ok(())
    }

    /// Finishes, as a reader, the shift claimed in object `name` (`bytes`,
    /// at `version`) when it stopped: a claim [`CLAIM_LAPSE`] old, read
    /// again just now as it was, the job written where it goes only if
    /// it isn't there, then the claim removed at its version. A claim
    /// younger, or gone or changed since, is its shift's own: left be.
    pub(super) fn settle_stopped(
        &self,
        name: &str,
        bytes: &[u8],
        version: &Version,
    ) -> anyhow::Result<()> {
        let JobHeld::Moving(moving) = held_job(bytes)? else {
            return Ok(());
        };
        if ((self.now)() / 1000).saturating_sub(moving.claimed) < CLAIM_LAPSE {
            return Ok(());
        }
        match self.store.get(name)? {
            Some((_, now)) if now == *version => {}
            _ => return Ok(()),
        }
        match self.store.put(
            &self.job(moving.moving_to, &moving.job.id),
            &placed(&moving.job, name, version)?,
            &Expect::Absent,
        ) {
            Err(err) if is_conflict(&err) => {}
            other => {
                other?;
            }
        }
        match self.store.delete(name, &Expect::Version(version.clone())) {
            Err(err) if is_conflict(&err) => {}
            other => other?,
        }
        Ok(())
    }

    /// Job `id` from `from` to `to`, whole. It's claimed in `from` first,
    /// written only if it's still the version read, so of two shifts of it
    /// one claims it; then written in `to`, then its claim removed. A
    /// shift that stopped between is finished by whoever reads its claim,
    /// so the job is never in neither place.
    pub(super) fn shift(&self, id: &str, from: Place, to: Place) -> anyhow::Result<bool> {
        let name = self.job(from, id);
        let Some((bytes, version)) = self.store.get(&name)? else {
            return Ok(false);
        };
        let job = match held_job(&bytes)? {
            JobHeld::Job(job) => job,
            JobHeld::Moving(_) => {
                self.settle_stopped(&name, &bytes, &version)?;
                return Ok(false);
            }
        };
        self.finish_claim(&bytes)?;
        let moving = Moving {
            moving_to: to,
            job,
            claimed: (self.now)() / 1000,
        };
        let claimed = match self.store.put(
            &name,
            &serde_json::to_vec(&moving)?,
            &Expect::Version(version),
        ) {
            Ok(claimed) => claimed,
            Err(err) if is_conflict(&err) => return Ok(false),
            Err(err) => return Err(err),
        };
        let bytes = serde_json::to_vec(&moving)?;
        self.settle(&name, &bytes, claimed)?;
        self.moved(from)?;
        self.moved(to)?;
        Ok(true)
    }

    /// The mark, and its version.
    pub(super) fn mark(&self) -> anyhow::Result<(u64, Option<Version>)> {
        Ok(match self.store.get("schedule/mark")? {
            None => (0, None),
            Some((bytes, version)) => (serde_json::from_slice(&bytes)?, Some(version)),
        })
    }

    /// The mark moved on, when `place` is waiting.
    pub(super) fn moved(&self, place: Place) -> anyhow::Result<()> {
        if place != Place::Waiting {
            return Ok(());
        }
        self.change("schedule/mark", |had| {
            let mark: u64 = match had {
                Some(bytes) => serde_json::from_slice(&bytes)?,
                None => 0,
            };
            Ok(Some(serde_json::to_vec(&(mark + 1))?))
        })?;
        Ok(())
    }
}
