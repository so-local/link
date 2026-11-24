//! The agent's scheduled jobs, in [`SCHEDULE`]: a job's file, read and
//! written, and the mark moved on with each change to what waits.

use super::*;

/// The folder in [`SCHEDULE`] of a job in `place`, and its file's name
/// there: an error for an ID that isn't one.
fn job_file(place: Place, id: &str) -> anyhow::Result<(&'static str, String)> {
    if !id.is_empty() {
        check_job_id(id)?;
    }
    Ok(match place {
        Place::Waiting => ("jobs", format!("{id}.json")),
        Place::Firing => ("firing", format!("{id}.json")),
        Place::Ending => ("firing", format!("{id}.ending")),
        Place::Ended => ("ended", format!("{id}.json")),
    })
}

/// An error unless `id` is a job's ID this folder keeps: one name, not
/// hidden, at most [`super::super::JOB_ID_MAX`] bytes.
fn check_job_id(id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        id.len() <= super::super::JOB_ID_MAX
            && !id.starts_with('.')
            && super::super::under::check_part(id).is_ok(),
        "{:?} isn't a job's ID",
        link_format::clean(id)
    );
    Ok(())
}

/// Job `id` from its file: the contract's [`Job`].
fn job_of(id: &str, bytes: &[u8]) -> anyhow::Result<Job> {
    let job: Job = serde_json::from_slice(bytes)
        .with_context(|| format!("reading job {}", link_format::clean(id)))?;
    anyhow::ensure!(
        job.id == id,
        "job {}'s file names another",
        link_format::clean(id)
    );
    Ok(job)
}

/// The mark [`MARK`] holds: 0 for none.
pub(super) fn mark_of(schedule: &Dir) -> anyhow::Result<u64> {
    Ok(match schedule.read(MARK, 32)? {
        Some(bytes) => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .context("the jobs' mark isn't a number")?,
        None => 0,
    })
}

impl Folder {
    /// The agent's `schedule/`, made if it isn't there.
    pub(super) fn schedule(&self) -> anyhow::Result<Dir> {
        self.root
            .walk(&self.schedule, true)?
            .context("the agent's schedule folder")
    }

    /// The folder of `place`'s jobs, in `schedule/`, and job `id`'s name
    /// there: the folder made when `make`, else none when it isn't there.
    pub(super) fn jobs(
        &self,
        place: Place,
        id: Option<&str>,
        make: bool,
    ) -> anyhow::Result<Option<(Dir, String)>> {
        let (folder, name) = job_file(place, id.unwrap_or_default())?;
        let schedule = match make {
            true => self.schedule()?,
            false => match self.root.walk(&self.schedule, false)? {
                Some(schedule) => schedule,
                None => return Ok(None),
            },
        };
        Ok(schedule.sub(folder, make)?.map(|dir| (dir, name)))
    }

    pub(super) fn job_put(&self, place: Place, job: &Job) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(job)?;
        anyhow::ensure!(
            bytes.len() as u64 <= JOB_MAX,
            "job {} is larger than {JOB_MAX} bytes",
            link_format::clean(&job.id)
        );
        let (dir, name) = self
            .jobs(place, Some(&job.id), true)?
            .context("the agent's jobs' folder")?;
        dir.write(&name, &bytes)?;
        if place == Place::Waiting {
            self.changed()?;
        }
        Ok(())
    }

    pub(super) fn job_get(&self, place: Place, id: &str) -> anyhow::Result<Option<Job>> {
        let Some((dir, name)) = self.jobs(place, Some(id), false)? else {
            return Ok(None);
        };
        dir.read(&name, JOB_MAX)?
            .map(|bytes| job_of(id, &bytes))
            .transpose()
    }

    pub(super) fn job_list(&self, place: Place) -> anyhow::Result<Vec<Job>> {
        let Some((dir, _)) = self.jobs(place, None, false)? else {
            return Ok(Vec::new());
        };
        let suffix = job_file(place, "")?.1;
        let mut jobs = Vec::new();
        for name in dir.names()? {
            let Some(id) = name.strip_suffix(suffix.as_str()) else {
                continue;
            };
            if check_job_id(id).is_err() {
                continue;
            }
            if let Some(bytes) = dir.read(&name, JOB_MAX)? {
                jobs.push(job_of(id, &bytes)?);
            }
        }
        Ok(jobs)
    }

    /// Job `id` moved from `from` to `to` by a rename, whole: of two
    /// shifts of it at once, one finds it.
    pub(super) fn job_shift(&self, id: &str, from: Place, to: Place) -> anyhow::Result<bool> {
        let Some((there, name)) = self.jobs(from, Some(id), false)? else {
            return Ok(false);
        };
        let (into, to_name) = self
            .jobs(to, Some(id), true)?
            .context("the agent's jobs' folder")?;
        match there.rename_into(&name, &into, &to_name) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(err.into()),
        }
        if from == Place::Waiting || to == Place::Waiting {
            self.changed()?;
        }
        Ok(true)
    }

    pub(super) fn job_remove(&self, place: Place, id: &str) -> anyhow::Result<bool> {
        let Some((dir, name)) = self.jobs(place, Some(id), false)? else {
            return Ok(false);
        };
        let found = dir.remove(&name)?;
        if found && place == Place::Waiting {
            self.changed()?;
        }
        Ok(found)
    }

    /// The mark moved on, one at a time.
    pub(super) fn changed(&self) -> anyhow::Result<()> {
        let schedule = self.schedule()?;
        let _locked = schedule.lock(libc::LOCK_EX)?;
        let next = mark_of(&schedule)?.wrapping_add(1);
        schedule.write(MARK, next.to_string().as_bytes())?;
        Ok(())
    }
}
