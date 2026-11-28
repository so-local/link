use crate::{approval::*, constants::*, delivery::*, *};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub question: QuestionSet,
    pub resolution: Option<Resolution>,
    pub effect: EffectState,
    pub answered_on: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub snapshot: Snapshot,
    pub code: String,
    pub idempotency: String,
    pub draft_digest: String,
    pub accepted: Option<(Ingress, ResponseCandidate)>,
    /// When the question was resolved (answered, expired, cancelled or
    /// closed), from which its [retention](crate::retention) counts.
    pub resolved_at: Option<u64>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct State {
    pub records: BTreeMap<RequestId, Record>,
    codes: BTreeMap<String, RequestId>,
    pub deliveries: BTreeMap<DeliveryId, Delivery>,
    #[serde(default)]
    pub messages: BTreeMap<String, DeliveryId>,
    #[serde(default)]
    pub drafts: BTreeMap<DeliveryId, crate::forms::Draft>,
    pub events: Vec<Event>,
    pub cursors: BTreeMap<String, u64>,
    #[serde(default)]
    pub controls: crate::control::ControlState,
}

link_files::specification!(State, include_str!("store.schema.toml"));

/// The approvals, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<State>()
}

impl State {
    /// Frees the response codes of the questions `gone`.
    pub(crate) fn forget_codes(&mut self, gone: &std::collections::BTreeSet<RequestId>) {
        self.codes.retain(|_, id| !gone.contains(id));
    }

    fn empty() -> Self {
        Self {
            records: BTreeMap::new(),
            codes: BTreeMap::new(),
            deliveries: BTreeMap::new(),
            messages: BTreeMap::new(),
            drafts: BTreeMap::new(),
            events: Vec::new(),
            cursors: BTreeMap::new(),
            controls: Default::default(),
        }
    }

    /// The state `bytes` hold, read from the folder at `at`. One it can't
    /// read is refused, its path and why in the error's own words: a
    /// caller that shows only an error's first line (a tool's answer)
    /// still says which file and what of it.
    fn parse(bytes: &[u8], at: &Path) -> anyhow::Result<Self> {
        link_files::saved::parse_json::<Self>(bytes).map_err(|err| {
            let file = at.join(STATE_FILE);
            anyhow::anyhow!(
                "invalid interaction state: {} isn't what this Link reads: {err:#}. \
                 Preserve this file before recovery: it also contains effects, deliveries, \
                 controls, and cursors. Reconcile outstanding operations before replacing state",
                file.display()
            )
        })
    }
}

/// Which file a state file is: its device and number there, size and
/// modification time. Every write replaces the file by rename, so a new
/// write is a new file. A number is reused only once its file is gone,
/// and [`Held`] keeps the file it read open, so the same number is the
/// same file; size and time catch an edit made in place by hand.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileId {
    dev: u64,
    ino: u64,
    size: u64,
    modified_ns: i64,
}

impl FileId {
    fn of(file: &std::fs::File) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata()?;
        Ok(Self {
            dev: meta.dev(),
            ino: meta.ino(),
            size: meta.size(),
            modified_ns: meta
                .mtime()
                .saturating_mul(1_000_000_000)
                .saturating_add(meta.mtime_nsec()),
        })
    }
}

/// The state file this process last read or wrote: the file, held open
/// so its number can't pass to another, and the state its bytes hold.
struct Held {
    _file: std::fs::File,
    id: FileId,
    bytes: Vec<u8>,
    state: Arc<State>,
}

/// What a [`Store`] and its clones last read, and how often they parsed.
#[derive(Default)]
struct Cache {
    held: Option<Held>,
    parses: u64,
}

impl Cache {
    /// The committed state in `dir`, the folder at `at`, parsed only when
    /// it's another file than the one held. The caller holds the folder's
    /// lock.
    fn current(
        &mut self,
        dir: &link_files::nofollow::Dir,
        at: &Path,
    ) -> anyhow::Result<Arc<State>> {
        let file = match dir.open_file(STATE_FILE.as_ref(), libc::O_RDONLY, 0) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.held = None;
                return Ok(Arc::new(State::empty()));
            }
            Err(e) => return Err(e.into()),
        };
        let id = FileId::of(&file)?;
        if let Some(held) = &self.held
            && held.id == id
        {
            return Ok(held.state.clone());
        }
        self.held = None;
        ensure!(
            id.size <= MAX_STATE_BYTES,
            "interaction state capacity exhausted"
        );
        let mut bytes = Vec::with_capacity(id.size as usize);
        std::io::Read::read_to_end(
            &mut std::io::Read::take(&file, MAX_STATE_BYTES + 1),
            &mut bytes,
        )?;
        ensure!(
            bytes.len() as u64 <= MAX_STATE_BYTES,
            "interaction state capacity exhausted"
        );
        self.parses += 1;
        let state = Arc::new(State::parse(&bytes, at)?);
        self.held = Some(Held {
            _file: file,
            id,
            bytes,
            state: state.clone(),
        });
        Ok(state)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitDisposition {
    Accepted,
    AlreadyResolved,
    Expired,
}

/// The interactions in one folder. A store and its clones share what
/// they last read, so a read of a file no one has written since parses
/// nothing.
#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
    cache: Arc<Mutex<Cache>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("dir", &self.dir).finish()
    }
}

impl Store {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            cache: Arc::default(),
        }
    }
    pub fn machine() -> anyhow::Result<Self> {
        Ok(Self::at(link_files::layout::Hidden::Approvals.try_path()?))
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Observe current committed state under the same directory lock as
    /// writers, without serializing, replacing or syncing the state file.
    /// A file no one wrote since this store last read it isn't parsed.
    pub(crate) fn read<T>(&self, f: impl FnOnce(&State) -> anyhow::Result<T>) -> anyhow::Result<T> {
        link_files::create_private_dir(&self.dir)?;
        let dir = link_files::nofollow::Dir::open_unlinked(&self.dir)?;
        let _held = dir.lock(libc::LOCK_SH)?;
        let state = self.cache().current(&dir, &self.dir)?;
        f(&state)
    }

    /// A single durable state write also commits the outbox. No lock crosses I/O to an adapter.
    /// What [`retention`](crate::retention) no longer keeps leaves in the same write, and a
    /// change that leaves the file's bytes as they were writes nothing.
    pub(crate) fn change<T>(
        &self,
        f: impl FnOnce(&mut State) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        link_files::create_private_dir(&self.dir)?;
        let dir = link_files::nofollow::Dir::open_unlinked(&self.dir)?;
        let _held = dir.lock(libc::LOCK_EX)?;
        let mut cache = self.cache();
        let current = cache.current(&dir, &self.dir)?;
        // Taken out while it changes: a change that fails leaves nothing
        // held, so the next call parses the file again.
        let held = cache.held.take();
        drop(current);
        let (mut state, before) = match held {
            Some(Held {
                _file: file,
                id,
                bytes,
                state,
            }) => {
                let state = match Arc::try_unwrap(state) {
                    Ok(state) => state,
                    Err(_) => State::parse(&bytes, &self.dir)?,
                };
                (state, Some((file, id, bytes)))
            }
            None => (State::empty(), None),
        };
        let result = f(&mut state)?;
        crate::retention::apply(&mut state, crate::now());
        ensure!(
            state.events.len() <= MAX_EVENTS,
            "interaction outbox capacity exhausted"
        );
        let bytes = link_files::saved::print_json(&state)?;
        ensure!(
            bytes.len() as u64 <= MAX_STATE_BYTES,
            "interaction state capacity exhausted"
        );
        if let Some((file, id, before)) = before
            && before == bytes
        {
            cache.held = Some(Held {
                _file: file,
                id,
                bytes,
                state: Arc::new(state),
            });
            return Ok(result);
        }
        // A crashed transaction's temporary file is never read as committed state.
        match dir.remove_file(TEMP_FILE.as_ref()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let mut temp = dir.create_new(TEMP_FILE.as_ref(), 0o600)?;
        temp.write_all(&bytes)?;
        temp.sync_all()?;
        dir.rename(TEMP_FILE.as_ref(), STATE_FILE.as_ref())?;
        dir.sync_all()?;
        cache.held = Some(Held {
            id: FileId::of(&temp)?,
            _file: temp,
            bytes,
            state: Arc::new(state),
        });
        Ok(result)
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        // A panic while it was held left at worst nothing held.
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn create(
        &self,
        requester: Requester,
        recipient: Actor,
        draft: QuestionDraft,
        now: u64,
    ) -> anyhow::Result<RequestId> {
        self.create_bound(
            requester,
            recipient,
            DomainId::new("question").map_err(anyhow::Error::msg)?,
            draft,
            Effect::ReplyOnly,
            None,
            now,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "trusted source context stays separate from the untrusted draft"
    )]
    pub fn create_bound(
        &self,
        requester: Requester,
        recipient: Actor,
        domain: DomainId,
        draft: QuestionDraft,
        effect: Effect,
        review: Option<PermissionReview>,
        now: u64,
    ) -> anyhow::Result<RequestId> {
        validation::text(&draft.caller_idempotency_key)?;
        let idempotency = serde_json::to_string(&(
            &requester.source,
            &requester.actor,
            &requester.work,
            &requester.tool_call,
            &draft.caller_idempotency_key,
        ))?;
        let digest = link_digest::sha256(&serde_json::to_vec(&(
            &requester, &recipient, &domain, &draft, &effect, &review,
        ))?);
        self.change(|state| {
            if let Some(record) = state
                .records
                .values()
                .find(|r| r.idempotency == idempotency)
            {
                ensure!(
                    record.draft_digest == digest,
                    "idempotency key reused with changed content"
                );
                return Ok(record.snapshot.question.id.clone());
            }
            ensure!(
                state.records.len() < MAX_RECORDS,
                "interaction capacity exhausted"
            );
            if let Some(work) = &requester.work {
                ensure!(
                    crate::runtime::claim_current(state, work),
                    "work changed before question admission"
                );
            }
            let id = RequestId::new(link_files::random_token()?).map_err(anyhow::Error::msg)?;
            let timeout = draft.requested_timeout.unwrap_or(EffectiveTimeout {
                after_secs: DEFAULT_WAIT_SECS,
                item_fallbacks: BTreeMap::new(),
                policy_revision: POLICY_REVISION.into(),
            });
            let question = QuestionSet {
                version: PROTOCOL.current,
                id: id.clone(),
                revision: 1,
                domain,
                requester,
                recipient,
                title: draft.title,
                questions: draft.questions,
                effect,
                review,
                expires_at: now
                    .checked_add(timeout.after_secs)
                    .context("deadline overflow")?,
                timeout,
                created_at: now,
            };
            validation::question(&question)?;
            let code = reserve_code(state, &id)?;
            let effect = if matches!(question.effect, Effect::ReplyOnly) {
                EffectState::None
            } else {
                EffectState::Awaiting
            };
            state.records.insert(
                id.clone(),
                Record {
                    snapshot: Snapshot {
                        question,
                        resolution: None,
                        effect,
                        answered_on: None,
                    },
                    code,
                    idempotency,
                    draft_digest: digest,
                    accepted: None,
                    resolved_at: None,
                },
            );
            event(state, Some(id.clone()), "created", now)?;
            Ok(id)
        })
    }

    pub fn snapshot(&self, id: &RequestId, now: u64) -> anyhow::Result<Snapshot> {
        self.change(|s| {
            expire(s, id, now)?;
            Ok(s.records
                .get(id)
                .context("unknown request")?
                .snapshot
                .clone())
        })
    }

    pub fn pending(&self, now: u64) -> anyhow::Result<Vec<Snapshot>> {
        self.change(|state| {
            for id in state.records.keys().cloned().collect::<Vec<_>>() {
                expire(state, &id, now)?;
            }
            Ok(state
                .records
                .values()
                .filter(|r| r.snapshot.resolution.is_none())
                .map(|r| r.snapshot.clone())
                .collect())
        })
    }

    pub fn plan_delivery(
        &self,
        id: &RequestId,
        route: &str,
        surface: &str,
        caps: &SurfaceCapabilities,
        anchor: Option<ContextAnchor>,
        now: u64,
    ) -> anyhow::Result<DeliveryEnvelope> {
        self.change(|state| {
            expire(state, id, now)?;
            let record = state.records.get(id).context("unknown request")?;
            ensure!(record.snapshot.resolution.is_none(), "question is closed");
            validation::delivery(&record.snapshot.question, caps)?;
            let delivery = state
                .deliveries
                .values()
                .find(|d| d.request == *id && d.route == route && d.surface == surface)
                .cloned()
                .unwrap_or(Delivery {
                    id: DeliveryId::new(link_files::random_token()?).map_err(anyhow::Error::msg)?,
                    request: id.clone(),
                    route: route.into(),
                    surface: surface.into(),
                    message: None,
                    anchor: anchor.filter(|_| caps.inline),
                    delivered: false,
                });
            let envelope = DeliveryEnvelope {
                question: record.snapshot.question.clone(),
                delivery: delivery.clone(),
                response_code: record.code.clone(),
            };
            state.deliveries.insert(delivery.id.clone(), delivery);
            Ok(envelope)
        })
    }

    pub fn delivered(&self, id: &DeliveryId, message: Option<String>) -> anyhow::Result<()> {
        self.delivered_ids(id, &message.into_iter().collect::<Vec<_>>())
    }

    pub fn delivered_ids(&self, id: &DeliveryId, messages: &[String]) -> anyhow::Result<()> {
        self.change(|state| {
            let delivery = state.deliveries.get_mut(id).context("unknown delivery")?;
            if delivery.delivered {
                ensure!(
                    delivery.message.as_ref() == messages.first(),
                    "delivery receipt changed"
                );
            }
            delivery.delivered = true;
            delivery.message = messages.first().cloned();
            for message in messages {
                validation::text(message)?;
                let key = serde_json::to_string(&(&delivery.route, &delivery.surface, message))?;
                if let Some(previous) = state.messages.insert(key, id.clone()) {
                    ensure!(
                        previous == *id,
                        "message is already bound to another question"
                    );
                }
            }
            Ok(())
        })
    }

    pub fn reply_delivery(
        &self,
        route: &str,
        surface: &str,
        message: &str,
    ) -> anyhow::Result<Option<Delivery>> {
        self.change(|s| {
            let key = serde_json::to_string(&(route, surface, message))?;
            Ok(s.messages
                .get(&key)
                .and_then(|id| s.deliveries.get(id))
                .cloned())
        })
    }

    pub fn submit(
        &self,
        ingress: &Ingress,
        candidate: ResponseCandidate,
        now: u64,
    ) -> anyhow::Result<SubmitDisposition> {
        self.change(|state| crate::forms::submit(state, ingress, candidate, now))
    }

    pub fn claim(
        &self,
        id: &RequestId,
        checks: &SourceChecks,
        now: u64,
    ) -> anyhow::Result<ClaimId> {
        self.change(|state| {
            if let Some(work) = &checks.operation.work {
                ensure!(
                    crate::runtime::claim_current(state, work),
                    "work control or attempt superseded permission"
                );
            }
            let record = state.records.get_mut(id).context("unknown request")?;
            link_approvals::eligible(
                &record.snapshot.question,
                &record.snapshot.effect,
                checks,
                now,
            )?;
            let claim = ClaimId::new(link_files::random_token()?).map_err(anyhow::Error::msg)?;
            record.snapshot.effect = EffectState::Applying {
                claim: claim.clone(),
            };
            if let Some(work) = &checks.operation.work
                && let Some(binding) = state.controls.runtime.work.get_mut(&work.attempt)
            {
                binding
                    .state
                    .in_flight_effects
                    .insert(checks.operation.id.clone());
            }
            event(state, Some(id.clone()), "applying", now)?;
            Ok(claim)
        })
    }

    pub fn report(
        &self,
        id: &RequestId,
        source: &SourceId,
        claim: &ClaimId,
        outcome: EffectOutcome,
        now: u64,
    ) -> anyhow::Result<()> {
        self.change(|state| {
            let record = state.records.get_mut(id).context("unknown request")?;
            ensure!(
                record.snapshot.question.requester.source == *source,
                "wrong effect source"
            );
            let next = link_approvals::outcome(&record.snapshot.effect, claim, outcome)?;
            if next == record.snapshot.effect {
                return Ok(());
            }
            record.snapshot.effect = next;
            let mut reconciled = None;
            if let Effect::Authorize(operation) = &record.snapshot.question.effect
                && !matches!(record.snapshot.effect, EffectState::Unknown { .. })
                && let Some(work) = &operation.work
                && let Some(binding) = state.controls.runtime.work.get_mut(&work.attempt)
            {
                binding.state.in_flight_effects.remove(&operation.id);
                reconciled = Some(work.attempt.clone());
            }
            if let Some(attempt) = reconciled {
                crate::runtime::reconciled(state, &attempt)?;
            }
            event(state, Some(id.clone()), "effect_outcome", now)
        })
    }

    pub fn cancel(
        &self,
        id: &RequestId,
        requester: &Requester,
        reason: &str,
        now: u64,
    ) -> anyhow::Result<()> {
        self.change(|state| {
            let record = state.records.get_mut(id).context("unknown request")?;
            ensure!(
                record.snapshot.question.requester == *requester,
                "wrong cancelling source"
            );
            close(record, reason);
            event(state, Some(id.clone()), "cancelled", now)
        })
    }

    pub fn events(&self, after: u64) -> anyhow::Result<Vec<Event>> {
        self.read(|s| Ok(s.events.iter().filter(|e| e.id > after).cloned().collect()))
    }
    pub fn cursor(&self, subscriber: &str) -> anyhow::Result<u64> {
        self.read(|s| Ok(s.cursors.get(subscriber).copied().unwrap_or_default()))
    }
    /// Registers `subscriber` and answers its cursor: from now on the
    /// outbox keeps every event after it until it acknowledges them. A
    /// new subscriber starts at the first event kept.
    pub fn subscribe(&self, subscriber: &str) -> anyhow::Result<u64> {
        validation::text(subscriber)?;
        self.change(|s| Ok(*s.cursors.entry(subscriber.into()).or_default()))
    }
    /// Forgets `subscriber`, which never reads again: the events only it
    /// would read leave in this write. Answers whether it was registered.
    pub fn unsubscribe(&self, subscriber: &str) -> anyhow::Result<bool> {
        self.change(|s| Ok(s.cursors.remove(subscriber).is_some()))
    }
    pub fn acknowledge(&self, subscriber: &str, event: u64) -> anyhow::Result<()> {
        self.change(|s| {
            ensure!(
                s.events.last().is_some_and(|e| e.id >= event),
                "unknown event cursor"
            );
            let cursor = s.cursors.entry(subscriber.into()).or_default();
            ensure!(event >= *cursor, "cursor cannot go backwards");
            *cursor = event;
            Ok(())
        })
    }
}

pub(crate) fn close(record: &mut Record, reason: &str) {
    if record.snapshot.resolution.is_none() {
        record.snapshot.resolution = Some(lifecycle::closed(&record.snapshot.question, reason));
    }
    if matches!(
        record.snapshot.effect,
        EffectState::Awaiting | EffectState::Authorized | EffectState::RequiresClarification
    ) {
        record.snapshot.effect = EffectState::Revoked {
            reason: reason.into(),
        };
    }
}

pub(crate) fn expire(state: &mut State, id: &RequestId, now: u64) -> anyhow::Result<()> {
    let record = state.records.get_mut(id).context("unknown request")?;
    if record.snapshot.resolution.is_none() && now >= record.snapshot.question.expires_at {
        record.snapshot.resolution = Some(lifecycle::timeout(&record.snapshot.question));
        if record.snapshot.effect == EffectState::Awaiting {
            record.snapshot.effect = EffectState::Denied;
        }
        event(state, Some(id.clone()), "expired", now)?;
    } else if now >= record.snapshot.question.expires_at
        && record.snapshot.effect == EffectState::Authorized
    {
        record.snapshot.effect = EffectState::Revoked {
            reason: "unclaimed permission expired".into(),
        };
        event(state, Some(id.clone()), "expired", now)?;
    }
    Ok(())
}

pub(crate) fn event(
    state: &mut State,
    request: Option<RequestId>,
    kind: &str,
    now: u64,
) -> anyhow::Result<()> {
    let id = state.events.last().map_or(1, |e| e.id + 1);
    state.events.push(Event {
        receipt: None,
        id,
        request,
        target: None,
        kind: kind.into(),
        at: now,
    });
    Ok(())
}

fn reserve_code(state: &mut State, id: &RequestId) -> anyhow::Result<String> {
    for _ in 0..CODE_ATTEMPTS {
        let mut code = String::new();
        while code.len() < CODE_LENGTH {
            let random = link_files::random_token()?;
            for bytes in random.as_bytes().as_chunks::<2>().0 {
                let byte = u8::from_str_radix(std::str::from_utf8(bytes)?, 16)? as usize;
                if byte < 256 - 256 % CODE_ALPHABET.len() && code.len() < CODE_LENGTH {
                    code.push(CODE_ALPHABET[byte % CODE_ALPHABET.len()] as char);
                }
            }
        }
        if !state.codes.contains_key(&code) {
            state.codes.insert(code.clone(), id.clone());
            return Ok(code);
        }
    }
    anyhow::bail!("response code namespace exhausted")
}

impl Store {
    pub fn find_code(&self, code: &str) -> anyhow::Result<Option<RequestId>> {
        self.read(|s| Ok(s.codes.get(&code.to_ascii_lowercase()).cloned()))
    }
    pub fn deliveries(&self, id: &RequestId) -> anyhow::Result<Vec<Delivery>> {
        self.read(|s| {
            Ok(s.deliveries
                .values()
                .filter(|d| d.request == *id)
                .cloned()
                .collect())
        })
    }

    /// Host-side observation. Waiting holds neither a transaction nor an ingress slot.
    #[expect(
        clippy::disallowed_methods,
        reason = "observes cross-process state without holding the store lock"
    )]
    pub fn wait(&self, id: &RequestId, cancelled: impl Fn() -> bool) -> anyhow::Result<Snapshot> {
        loop {
            let snapshot = self.snapshot(id, now())?;
            if snapshot.resolution.is_some() {
                return Ok(snapshot);
            }
            if cancelled() {
                self.cancel(id, &snapshot.question.requester, "source cancelled", now())?;
                return self.snapshot(id, now());
            }
            std::thread::sleep(POLL);
        }
    }
}

#[cfg(test)]
impl Store {
    /// How many times this store and its clones parsed the state file.
    pub(crate) fn parses(&self) -> u64 {
        self.cache().parses
    }
}
