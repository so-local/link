//! The [`Store`]: the jobs, waiting and being fired, the day's count, the
//! runners' mark and the posts' turn.

use super::*;

/// Whether a job's dependencies ([`Job::after`]) let it fire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dependencies {
    /// Each ended done, or it has none.
    Ready,
    /// One hasn't ended yet.
    Waiting,
    /// One failed, was cancelled, or is gone: it never fires.
    Broken(String),
}
const JOBS_CONTROL: &str = "jobs-control.lock";

/// The store: the agent's jobs, kept by a [`JobStore`] (a folder of the
/// agent's, [`store::Stored`], unless the machine gives another), and a
/// folder of this machine's beside them for what only this machine
/// needs: the runners' mark ([`Store::runner`]), the jobs started in the
/// agent's environment ([`jobs`]), and the posts' turn
/// ([`Address::post`]).
#[derive(Clone)]
pub struct Store {
    pub(crate) interactions: Option<link_interactions::Store>,
    pub(crate) interaction_agent: Option<String>,
    dir: PathBuf,
    jobs: std::sync::Arc<dyn JobStore>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl Store {
    /// Its folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The jobs in folder `dir` ([`store::Stored`]).
    pub fn new(dir: &Path) -> anyhow::Result<Self> {
        let folder = store::Stored::file(dir)?;
        Ok(Self {
            interactions: None,
            interaction_agent: None,
            dir: dir.to_path_buf(),
            jobs: std::sync::Arc::new(folder),
        })
    }

    /// The jobs `jobs` keeps, and folder `dir` of this machine's for the
    /// rest.
    pub fn with(dir: &Path, jobs: std::sync::Arc<dyn JobStore>) -> anyhow::Result<Self> {
        link_files::create_private_dir(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        Ok(Self {
            interactions: None,
            interaction_agent: None,
            dir: dir.to_path_buf(),
            jobs,
        })
    }

    /// Keeps `job`, whole.
    pub fn put(&self, job: &Job) -> anyhow::Result<()> {
        let _lock = link_files::lock_file(&self.dir.join(JOBS_CONTROL))?;
        self.put_retained(job)
    }

    fn put_retained(&self, job: &Job) -> anyhow::Result<()> {
        if job.kind == Kind::Task
            && let (Some(store), Some(agent)) = (&self.interactions, &self.interaction_agent)
        {
            anyhow::ensure!(
                store
                    .work_status(agent, &job.thread)?
                    .is_none_or(|s| !matches!(
                        s.lifecycle,
                        link_interactions::control::Lifecycle::Deleting
                            | link_interactions::control::Lifecycle::Deleted
                    )),
                "task record is being deleted"
            );
        }
        let mut job = job.clone();
        if job.kind == Kind::Schedule
            && job.portable.is_none()
            && let (Some(interactions), Some(agent)) = (&self.interactions, &self.interaction_agent)
        {
            let occurrence = job.occurrence.get_or_insert_with(|| {
                link_agent_state_protocol::schedules::ScheduledOccurrence {
                    schedule: link_agent_state_protocol::schedules::ScheduleId(format!(
                        "tool:{}",
                        job.id
                    )),
                    revision: 1,
                    scheduled_at: job.due,
                }
            });
            interactions.register_schedule(
                agent,
                &occurrence.schedule,
                occurrence.revision,
                Some(job.thread.clone()),
            )?;
        }
        self.jobs.put(&job)
    }

    pub fn with_interactions(mut self, store: link_interactions::Store) -> Self {
        self.interactions = Some(store);
        self
    }

    pub fn with_interaction_agent(mut self, agent: Option<String>) -> Self {
        self.interaction_agent = agent;
        self
    }

    /// Job `id`, waiting, if it's there.
    pub fn get(&self, id: &str) -> Option<Job> {
        self.jobs.get(Place::Waiting, id)
    }

    /// Job `id` ended, with its outcome, if it's kept.
    pub fn ended(&self, id: &str) -> Option<Job> {
        self.jobs.get(Place::Ended, id)
    }

    /// Every job ended and kept, longest ended first.
    pub fn ended_jobs(&self) -> Vec<Job> {
        let mut jobs = self.jobs.list(Place::Ended);
        jobs.sort_by_key(|job| (job.outcome.as_ref().map_or(0, |o| o.at), job.id.clone()));
        jobs
    }

    /// Cancels job `id`, waiting or being fired, and with it every job it
    /// coordinates ([`Job::parent`]) and every job waiting on it
    /// ([`Job::after`]), each kept ended as cancelled. A subagent at work
    /// finishes the turn it's in, but what it answers is never kept, and
    /// nothing it started fires again. Whether it was there.
    pub fn cancel(&self, id: &str) -> anyhow::Result<bool> {
        link_machine::machine::check_id(id)?;
        let _lock = link_files::lock_file(&self.dir.join(JOBS_CONTROL))?;
        let mut cancelled = self.cancel_locked(id, "cancelled", now(), true)?;
        // A subagent that answered and ended done: what it left waiting
        // goes with it, and it's kept cancelled, so it starts nothing more.
        if !cancelled
            && let Some(ended) = self.ended(id)
            && matches!(
                ended.outcome.as_ref().map(|o| &o.how),
                Some(How::Done { .. })
            )
        {
            self.keep_ended(
                ended,
                How::Cancelled {
                    why: "cancelled after it answered".into(),
                },
                now(),
            )?;
            self.cascade(id, now())?;
            cancelled = true;
        }
        self.prune(now());
        Ok(cancelled)
    }

    /// Deletes job `id`: cancelled as [`Store::cancel`] cancels it, with
    /// what it coordinates and what waits on them, and then gone, kept
    /// neither waiting nor ended, so no list shows it. One that already
    /// ended is taken out of what's kept. Whether it was there.
    pub fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let cancelled = self.cancel(id)?;
        let _lock = link_files::lock_file(&self.dir.join(JOBS_CONTROL))?;
        let removed = self.jobs.remove(Place::Ended, id);
        Ok(cancelled || removed)
    }

    /// [`Store::cancel`], under the jobs' lock, `why` kept as its reason.
    /// A subagent at work whose host admitted it is the host's to stop:
    /// asked `directly`, refused; reached by a cascade, left to it.
    fn cancel_locked(&self, id: &str, why: &str, at: u64, directly: bool) -> anyhow::Result<bool> {
        let place = [Place::Waiting, Place::Firing]
            .into_iter()
            .find(|place| self.jobs.get(*place, id).is_some());
        let Some(place) = place else {
            return Ok(false);
        };
        let Some(job) = self.jobs.get(place, id) else {
            return Ok(false);
        };
        if place == Place::Firing && self.admitted(&job)? {
            match directly {
                true => {
                    bail!("subagent {id} is at work, admitted by its host: its answer is coming")
                }
                false => return Ok(false),
            }
        }
        // Taken first, so a runner settling it meanwhile finds it gone.
        if !self.jobs.remove(place, id) {
            return Ok(false);
        }
        self.keep_ended(
            job,
            How::Cancelled {
                why: why.to_string(),
            },
            at,
        )?;
        self.cascade(id, at)?;
        Ok(true)
    }

    /// Cancels what ended job `id` leaves unable to go on: the jobs it
    /// coordinated, and those waiting on it.
    fn cascade(&self, id: &str, at: u64) -> anyhow::Result<()> {
        let related: Vec<(String, String)> = self
            .jobs()
            .into_iter()
            .chain(self.firing())
            .filter_map(|job| {
                if job.parent.as_deref() == Some(id) {
                    Some((job.id, format!("its coordinator {id} ended without it")))
                } else if job.after.iter().any(|a| a == id) {
                    Some((job.id, format!("job {id} it waits on didn't end done")))
                } else {
                    None
                }
            })
            .collect();
        for (child, why) in related {
            self.cancel_locked(&child, &why, at, false)?;
        }
        Ok(())
    }

    /// Whether `job`, a subagent, is work its interactions host admitted
    /// and hasn't ended: none without a host.
    fn admitted(&self, job: &Job) -> anyhow::Result<bool> {
        let (Some(store), Some(agent)) = (&self.interactions, &self.interaction_agent) else {
            return Ok(false);
        };
        if job.kind != Kind::Task {
            return Ok(false);
        }
        Ok(store.work_status(agent, &job.thread)?.is_some_and(|s| {
            !matches!(
                s.lifecycle,
                link_interactions::control::Lifecycle::Archived
                    | link_interactions::control::Lifecycle::Deleting
                    | link_interactions::control::Lifecycle::Deleted
            )
        }))
    }

    /// Keeps `job` ended as `how` at `at`, and wakes the runner when a
    /// job waits on it.
    fn keep_ended(&self, mut job: Job, how: How, at: u64) -> anyhow::Result<()> {
        job.outcome = Some(Outcome { at, how });
        self.jobs.put_ended(&job)?;
        // A waiting job put again moves the mark: the runner looks now.
        for waiting in self.jobs() {
            if waiting.after.contains(&job.id) {
                self.jobs.put(&waiting)?;
            }
        }
        Ok(())
    }

    /// Whether `job`'s dependencies let it fire now.
    pub fn dependencies(&self, job: &Job) -> Dependencies {
        for id in &job.after {
            if let Some(ended) = self.ended(id) {
                match ended.outcome.map(|o| o.how) {
                    Some(How::Done { .. }) => continue,
                    Some(How::Failed { .. }) => {
                        return Dependencies::Broken(format!("job {id} it waits on failed"));
                    }
                    _ => {
                        return Dependencies::Broken(format!("job {id} it waits on was cancelled"));
                    }
                }
            }
            let pending = [Place::Waiting, Place::Firing, Place::Ending]
                .into_iter()
                .any(|place| self.jobs.get(place, id).is_some());
            match pending {
                true => return Dependencies::Waiting,
                false => {
                    return Dependencies::Broken(format!("job {id} it waits on is gone"));
                }
            }
        }
        Dependencies::Ready
    }

    /// Whether job `id` may still start work of its own: waiting, being
    /// fired, or ended done (a coordinator whose answers come in after
    /// its turn). Not once it was cancelled, failed or is gone, so nothing
    /// it starts outlives its cancelling.
    pub(crate) fn at_work(&self, id: &str) -> bool {
        if let Some(ended) = self.ended(id) {
            return matches!(ended.outcome.map(|o| o.how), Some(How::Done { .. }));
        }
        [Place::Waiting, Place::Firing, Place::Ending]
            .into_iter()
            .any(|place| self.jobs.get(place, id).is_some())
    }

    /// Ends job `id`, waiting, as `why` says, its dependencies broken:
    /// kept cancelled, with what it coordinates and what waits on it.
    pub(super) fn break_off(&self, id: &str, why: &str) -> anyhow::Result<bool> {
        link_machine::machine::check_id(id)?;
        let _lock = link_files::lock_file(&self.dir.join(JOBS_CONTROL))?;
        self.cancel_locked(id, why, now(), true)
    }

    /// Forgets the ended jobs kept past [`Limits::ended_keep_secs`] at `at`, and the
    /// longest ended past [`Limits::ended_max`].
    pub fn prune(&self, at: u64) {
        let ended = self.ended_jobs();
        let over = ended.len().saturating_sub(limits().ended_max);
        for (n, job) in ended.into_iter().enumerate() {
            let when = job.outcome.as_ref().map_or(0, |o| o.at);
            if n < over || at.saturating_sub(when) > limits().ended_keep_secs {
                self.jobs.remove(Place::Ended, &job.id);
            }
        }
    }

    pub(crate) fn remove_retained(&self, id: &str) -> anyhow::Result<()> {
        link_machine::machine::check_id(id)?;
        let _lock = link_files::lock_file(&self.dir.join(JOBS_CONTROL))?;
        for place in [Place::Waiting, Place::Firing, Place::Ending, Place::Ended] {
            self.jobs.remove(place, id);
        }
        Ok(())
    }

    fn read(&self, place: Place) -> Vec<Job> {
        let mut jobs = self.jobs.list(place);
        jobs.sort_by_key(|job: &Job| (job.due, job.id.clone()));
        jobs
    }

    /// Every job waiting, soonest first.
    pub fn jobs(&self) -> Vec<Job> {
        self.read(Place::Waiting)
    }

    /// When the soonest job waiting is due: none if none is; an error
    /// when its store can't say ([`JobStore::next`]).
    pub fn next(&self) -> anyhow::Result<Option<u64>> {
        self.jobs.next()
    }

    /// How many jobs are waiting, none read.
    pub fn waiting(&self) -> usize {
        self.jobs.count(Place::Waiting)
    }

    /// Every job being fired.
    pub fn firing(&self) -> Vec<Job> {
        self.read(Place::Firing)
    }

    /// Takes job `id` to fire it, moving it aside: whether it was still
    /// there (not cancelled, nor taken by another runner).
    pub(super) fn claim(&self, id: &str) -> bool {
        self.jobs.shift(id, Place::Waiting, Place::Firing)
    }

    /// Ends the firing of job `id`: it waits again as `then`, or is gone.
    /// Nothing if it was cancelled meanwhile; kept aside if `then` can't
    /// be written, so it can't fire again before the runner restarts.
    pub(super) fn settle(&self, id: &str, then: Option<&Job>) -> anyhow::Result<()> {
        link_machine::machine::check_id(id)?;
        let _lock = link_files::lock_file(&self.dir.join(JOBS_CONTROL))?;
        // Taken from being fired first, so a cancel is either before (and
        // wins) or after (and finds it waiting again).
        if !self.jobs.shift(id, Place::Firing, Place::Ending) {
            return Ok(());
        }
        if let Some(job) = then
            && let Err(err) = self.put_retained(job)
        {
            self.jobs.shift(id, Place::Ending, Place::Firing);
            return Err(err);
        }
        self.jobs.remove(Place::Ending, id);
        Ok(())
    }

    /// Ends the firing of job `id` for good, kept ended as `how`: what
    /// it coordinated still waiting is cancelled once it didn't end done,
    /// and what waits on it is woken. Nothing if it was cancelled
    /// meanwhile, which wins.
    pub(super) fn finish(&self, id: &str, how: How) -> anyhow::Result<()> {
        link_machine::machine::check_id(id)?;
        let _lock = link_files::lock_file(&self.dir.join(JOBS_CONTROL))?;
        if !self.jobs.shift(id, Place::Firing, Place::Ending) {
            return Ok(());
        }
        let Some(job) = self.jobs.get(Place::Ending, id) else {
            return Ok(());
        };
        let done = matches!(how, How::Done { .. });
        if let Err(err) = self.keep_ended(job, how, now()) {
            self.jobs.shift(id, Place::Ending, Place::Firing);
            return Err(err);
        }
        self.jobs.remove(Place::Ending, id);
        if !done {
            self.cascade(id, now())?;
        }
        self.prune(now());
        Ok(())
    }

    /// The jobs a stopped runner was firing, waiting again: a restart
    /// loses none, though one cut off as it went may be sent twice. Each
    /// counts as a try, so one that stops the runner each time is given
    /// up.
    pub fn recover(&self) {
        // What a runner before these kept: its subagents' marks.
        let _ = std::fs::remove_dir_all(self.dir.join("running"));
        // A firing stopped as it ended: being fired, again.
        for job in self.jobs.list(Place::Ending) {
            self.jobs.shift(&job.id, Place::Ending, Place::Firing);
        }
        for job in self.firing() {
            let job = Job {
                tries: job.tries + 1,
                ..job
            };
            let then = match job.tries > limits().retries {
                true => {
                    warn(sentence!(
                        "The job from {} was cut off {} and given up on.",
                        Foreign::new(&job.from()),
                        Count::new(job.tries.into(), "time", "times")
                    ));
                    None
                }
                false => Some(&job),
            };
            if let Err(err) = self.settle(&job.id, then) {
                warn_why(
                    sentence!(
                        "The job from {} couldn't be updated.",
                        Foreign::new(&job.from())
                    ),
                    &err,
                );
            }
        }
    }

    /// How many messages went on the day holding `at`, in the agent's
    /// time zone.
    pub fn sent_on(&self, at: u64) -> u32 {
        let (day, sent) = self.jobs.day();
        match day == local_day(at) {
            true => sent,
            false => 0,
        }
    }

    /// Counts one message sent on the day holding `at`.
    pub(super) fn count(&self, at: u64) -> anyhow::Result<()> {
        self.jobs.set_day(local_day(at), self.sent_on(at) + 1)
    }

    /// How many subagents are at work: their tasks waiting, or fired and
    /// not ended.
    /// Each job's kind only is read ([`JobStore::kinds`]), its text never.
    pub fn subagents(&self) -> u32 {
        if let (Some(interactions), Some(agent)) = (&self.interactions, &self.interaction_agent) {
            let mut ids: std::collections::BTreeSet<_> = self
                .jobs()
                .into_iter()
                .chain(self.firing())
                .filter(|j| j.kind == Kind::Task)
                .map(|j| j.id)
                .collect();
            let Ok(work) = interactions.works(agent) else {
                return u32::MAX;
            };
            for binding in work {
                if let link_interactions::WorkKind::Child { id } = &binding.state.work.kind {
                    if interactions
                        .child_result_delivered(agent, &binding.state.work.session)
                        .unwrap_or(false)
                        || matches!(
                            binding.state.lifecycle,
                            link_interactions::control::Lifecycle::Archived
                                | link_interactions::control::Lifecycle::Deleted
                        )
                    {
                        ids.remove(id);
                    } else {
                        ids.insert(id.clone());
                    }
                }
            }
            return ids.len() as u32;
        }
        // One ending is still at work until its outcome is kept.
        [Place::Waiting, Place::Firing, Place::Ending]
            .into_iter()
            .flat_map(|place| self.jobs.kinds(place))
            .filter(|kind| *kind == Kind::Task)
            .count() as u32
    }

    /// A mark that differs once a job is put, taken or cancelled
    /// ([`JobStore::changed`]).
    pub fn changed(&self) -> Option<u64> {
        self.jobs.changed()
    }

    /// Waits until `until` (a minute at most), or until a job is put,
    /// taken or cancelled since the mark was `changed`
    /// ([`Store::changed`], taken before the jobs were read, so a job put
    /// meanwhile isn't missed): the mark, looked at once a second, reads
    /// no job.
    pub fn wait(&self, until: Option<u64>, changed: Option<u64>) {
        self.wait_while(until, changed, &|| true)
    }

    /// [`Store::wait`], or until `keep` says no.
    #[expect(clippy::disallowed_methods, reason = "the scheduler's own period")]
    pub fn wait_while(&self, until: Option<u64>, changed: Option<u64>, keep: &dyn Fn() -> bool) {
        let until = until
            .unwrap_or(u64::MAX)
            .min(now() + limits().look_at_most_secs);
        loop {
            std::thread::sleep(limits().look_every());
            if now() >= until || self.changed() != changed || !keep() {
                return;
            }
        }
    }

    /// Marks a runner at work on the store until the mark is dropped (a
    /// lock on `runner.lock`, let go when its process ends, however it
    /// ends). The first, alone, moves back what a stopped one was firing
    /// ([`Store::recover`]); another beside it leaves alone what that one
    /// fires: a runner the calendar wakes each minute may still be
    /// waiting on a firing when the next starts.
    pub fn runner(&self) -> anyhow::Result<std::fs::File> {
        use std::os::fd::AsRawFd;
        let file = self.lock_file("runner.lock")?;
        // SAFETY: flock takes only the file's descriptor, open while
        // `file` lives.
        let alone = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        if alone {
            self.recover();
        }
        // Shared from here: another runner may start beside it.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH) } != 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("locking {}", self.dir.join("runner.lock").display()));
        }
        Ok(file)
    }

    /// The posts' turn ([`Address::post`]), held until it's dropped (a
    /// lock on `post.lock`): one post at a time, each reading the jobs
    /// once it has its turn, so the last one made says what's kept last.
    pub(super) fn posting(&self) -> anyhow::Result<std::fs::File> {
        use std::os::fd::AsRawFd;
        let file = self.lock_file("post.lock")?;
        // SAFETY: as in [`Store::runner`].
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("locking {}", self.dir.join("post.lock").display()));
        }
        Ok(file)
    }

    /// Lock file `name` in its folder, opened in place, never through a
    /// link (`O_NOFOLLOW`): one put at its name, by whoever else writes
    /// the folder, leads nowhere.
    fn lock_file(&self, name: &str) -> anyhow::Result<std::fs::File> {
        use std::os::unix::fs::OpenOptionsExt;
        let path = self.dir.join(name);
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::*;

    /// A link put at a lock's name is never followed: the runner and the
    /// posts refuse it, and make nothing where it points.
    #[test]
    fn a_lock_is_never_reached_through_a_link() {
        let (dir, store) = store("lock-link");
        let target = dir.join("elsewhere");
        for name in ["runner.lock", "post.lock"] {
            std::os::unix::fs::symlink(&target, store.dir.join(name)).unwrap();
        }
        assert!(store.runner().is_err());
        assert!(store.posting().is_err());
        assert!(!target.exists(), "a lock made through its link");
        for name in ["runner.lock", "post.lock"] {
            std::fs::remove_file(store.dir.join(name)).unwrap();
        }
        assert!(store.runner().is_ok());
        assert!(store.posting().is_ok());
    }

    /// Jobs are counted by their files, and subagents by each file's
    /// kind, read from its start; a file written otherwise is read whole.
    #[test]
    fn jobs_are_counted_without_reading_them() {
        let (dir, store) = store("counted");
        assert_eq!((store.waiting(), store.subagents()), (0, 0));
        let id = |n: u32| format!("job{n:09}");
        for n in 0..5 {
            let mut job = job(&id(n), 100);
            if n % 2 == 0 {
                job.kind = Kind::Task;
                job.text = "x".repeat(64 << 10);
            }
            store.put(&job).unwrap();
        }
        assert_eq!((store.waiting(), store.subagents()), (5, 3));
        // Written by hand, its fields in another order: read whole.
        let mut by_hand = serde_json::to_value(job(&id(7), 100)).unwrap();
        by_hand["kind"] = serde_json::json!("task");
        let kept = serde_json::json!({"due": 100, "spec": by_hand, "id": id(7)});
        let pretty = serde_json::to_vec_pretty(&kept).unwrap();
        std::fs::create_dir_all(dir.join("schedule/firing")).unwrap();
        std::fs::write(
            dir.join("schedule/firing").join(format!("{}.json", id(7))),
            pretty,
        )
        .unwrap();
        assert_eq!(store.subagents(), 4);
        // Being fired isn't waiting; a file aside isn't a job.
        assert_eq!(store.waiting(), 5);
        std::fs::write(dir.join("schedule/jobs/x.123.tmp"), "{}").unwrap();
        assert_eq!(store.waiting(), 5);
        assert_eq!(store.jobs().len(), store.waiting());
    }
}
