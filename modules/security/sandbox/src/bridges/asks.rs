//! Bound source operations reviewed through the shared interaction engine.
use link_interactions::{approval::*, delivery::DeliveryEnvelope, sources, *};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const MOST: Duration = Duration::from_secs(24 * 60 * 60);
const POLL: Duration = Duration::from_millis(200);
const CAPABILITY_LIMIT: &str =
    "The external program's actual effects are not verified by its description or hints.";

/// Resource-owned proposal. Exact payload bytes, not the wording, bind dispatch.
#[derive(Clone, Debug)]
pub struct Proposal {
    pub call: Option<String>,
    pub work: Option<WorkRef>,
    pub sandbox: String,
    pub agent: Option<String>,
    pub capability: String,
    pub details: Vec<String>,
    pub payload: Vec<u8>,
}
impl Proposal {
    pub fn sandbox(name: &str, capability: impl Into<String>, details: Vec<String>) -> Self {
        let capability = capability.into();
        Self {
            call: None,
            work: None,
            sandbox: name.into(),
            agent: None,
            payload: capability.as_bytes().to_vec(),
            capability,
            details,
        }
    }
    pub fn for_agent(mut self, agent: Option<&str>) -> Self {
        self.agent = agent.map(str::to_string);
        self
    }
    pub fn review(&self) -> PermissionReview {
        PermissionReview {
            capability: self.capability.clone(),
            targets: vec![self.sandbox.clone()],
            lifetime: "This exact operation, once".into(),
            consequences: vec![CAPABILITY_LIMIT.into()],
            requester_reason: None,
            exact_operation: self.details.join("\n\n"),
        }
    }
    pub fn text(&self) -> String {
        // Text rendering visibly escapes controls and always retains full scope.
        let mut text = format!(
            "{} wants to {}.\n\n{}",
            self.sandbox,
            self.capability,
            self.details.join("\n\n")
        );
        text = text
            .chars()
            .flat_map(|c| {
                if c.is_control() && c != '\n' && c != '\t' {
                    c.escape_unicode().collect::<Vec<_>>()
                } else {
                    vec![c]
                }
            })
            .collect();
        text
    }
}

pub fn timeout() -> Duration {
    if cfg!(test) {
        Duration::from_secs(2)
    } else {
        crate::settings::get().ask_wait
    }
}
pub fn wait_of(asking: Option<&link_sandbox_client::routes::Asking>) -> Duration {
    asking
        .map(|a| Duration::from_secs(a.wait_secs).clamp(Duration::from_secs(1), MOST))
        .unwrap_or_else(timeout)
}
/// The machine's interaction store ([`Store::machine`]), where its
/// owner's questions wait and are delivered, answered and caught on every
/// surface: its approvals ([`Hidden::Approvals`]) in the installation folder
/// whose sandboxes folder ([`Hidden::Sandboxes`]) is `dir`, as the bridge's
/// settings or this process's own place it. Refused for a `dir` that is
/// no installation folder's sandboxes folder: a store of its own would hold
/// questions no other surface reads.
///
/// [`Hidden::Approvals`]: link_files::layout::Hidden::Approvals
/// [`Hidden::Sandboxes`]: link_files::layout::Hidden::Sandboxes
pub fn store(dir: &Path) -> anyhow::Result<Store> {
    use link_files::layout::Hidden;
    let depth = Path::new(Hidden::Sandboxes.relative()).components().count();
    let machine = dir.ancestors().nth(depth);
    match machine.filter(|machine| Hidden::Sandboxes.at(machine) == dir) {
        Some(machine) => Ok(Store::at(Hidden::Approvals.at(machine))),
        None => anyhow::bail!(
            "{} isn't an installation folder's sandboxes folder, so it has no interaction store",
            dir.display()
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Allowed,
    Refused,
    Unanswered,
    Unasked,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waits {
    Always,
    IfAnswered,
}

pub trait Dialog: Sync {
    fn show(
        &self,
        text: &str,
        desktop: &[(String, String)],
        wait: Duration,
        cancel: &AtomicBool,
    ) -> Option<Answer>;
    fn response(
        &self,
        store: &Store,
        envelope: &DeliveryEnvelope,
        desktop: &[(String, String)],
        wait: Duration,
        cancel: &AtomicBool,
    ) -> Option<ResponseValue> {
        let yes = match self.show(&render::delivery(envelope), desktop, wait, cancel)? {
            Answer::Allowed => true,
            Answer::Refused => false,
            _ => return None,
        };
        let _ = store;
        Some(ResponseValue::Submit(SetSubmission {
            answers: std::collections::BTreeMap::from([(
                envelope.question.questions[0].id.clone(),
                ItemAnswer::Confirmation { yes },
            )]),
            comment: None,
        }))
    }
}
pub struct Desktop;
impl Dialog for Desktop {
    fn show(&self, _: &str, _: &[(String, String)], _: Duration, _: &AtomicBool) -> Option<Answer> {
        None
    }
    fn response(
        &self,
        store: &Store,
        envelope: &DeliveryEnvelope,
        desktop: &[(String, String)],
        _: Duration,
        cancel: &AtomicBool,
    ) -> Option<ResponseValue> {
        use link_interactions::native::Dialog as _;
        if cfg!(test) {
            return None;
        }
        let cancelled = || {
            cancel.load(Ordering::Relaxed)
                || store
                    .snapshot(&envelope.question.id, now())
                    .map_or(true, |s| s.resolution.is_some())
        };
        let review = format!(
            "{}\n\nIn this dialog omit the response code. Enter yes or no, and include any qualification. Submit sends the answer once.",
            render::delivery(envelope)
        );
        let text = native::Session(desktop)
            .show(store, &review, &cancelled)
            .ok()??;
        reply::parse(
            &envelope.question,
            &text,
            false,
            false,
            text.trim() == "/dismiss",
        )
        .ok()
    }
}

/// A positive response still needs the source's fresh checks and one-shot claim.
pub struct Reviewed {
    pub answer: Answer,
    store: Store,
    request: RequestId,
    operation: BoundOperation,
}
impl Reviewed {
    /// Preserve the owner's complete typed response, including a qualifying yes or comment.
    pub fn refusal(&self) -> String {
        match self
            .store
            .snapshot(&self.request, now())
            .and_then(|s| Ok(serde_json::to_string(&s.resolution)?))
        {
            Ok(resolution) => {
                format!("The reviewed operation was not authorized. Owner resolution: {resolution}")
            }
            Err(err) => format!(
                "The reviewed operation was not authorized; its resolution could not be read: {err:#}"
            ),
        }
    }
    pub fn claim(self, payload: &[u8], authority_current: bool) -> anyhow::Result<Application> {
        anyhow::ensure!(
            self.answer == Answer::Allowed,
            "the reviewed operation was not allowed"
        );
        let reviewed = self
            .store
            .reviewed_effect(&self.request, &self.operation.source)?;
        anyhow::ensure!(
            reviewed.operation() == &self.operation,
            "the reviewed operation changed"
        );
        reviewed.claim(payload, authority_current)
    }
}
pub use sources::Application;

pub fn ask(
    dir: &Path,
    proposal: &Proposal,
    desktop: &[(String, String)],
    waits: Waits,
    wait: Duration,
) -> anyhow::Result<Reviewed> {
    ask_with(&Desktop, dir, proposal, desktop, waits, wait)
}

pub fn ask_with(
    dialog: &dyn Dialog,
    dir: &Path,
    proposal: &Proposal,
    desktop: &[(String, String)],
    waits: Waits,
    wait: Duration,
) -> anyhow::Result<Reviewed> {
    let store = store(dir)?;
    // Source-scoped serialization, independent of the engine transaction lock.
    let _one = link_files::lock_file(&store.dir().join(format!(
        "source-{}.lock",
        link_digest::sha256(proposal.sandbox.as_bytes())
    )))?;
    let review = proposal.review();
    let mut requester = sources::requester(
        "sandbox",
        // A sandbox runs for this machine's owner, and asks as them.
        link_interactions::OWNER,
        &proposal.sandbox,
    )?;
    requester.routing_agent = proposal.agent.clone();
    requester.work = proposal.work.clone();
    requester.tool_call = proposal.call.clone();
    let mut operation = sources::operation(
        requester.source.clone(),
        &proposal.payload,
        &review,
        Scope::OperationOnce,
        proposal.work.clone(),
    )?;
    if let (Some(work), Some(call)) = (&proposal.work, &proposal.call) {
        operation.id = OperationId::new(link_digest::sha256(&serde_json::to_vec(&(
            &requester.source,
            &proposal.sandbox,
            &proposal.capability,
            work,
            call,
        ))?))
        .map_err(anyhow::Error::msg)?;
    }
    let draft = sources::confirmation(
        proposal.capability.clone(),
        "Allow this exact operation?".into(),
        operation.id.to_string(),
        wait,
    );
    let request = store.create_bound(
        requester.clone(),
        sources::owner(),
        DomainId::new("tool.operation").map_err(anyhow::Error::msg)?,
        draft,
        Effect::Authorize(Box::new(operation.clone())),
        Some(review),
        now(),
    )?;
    // Read after the question is out: the owner may already have answered
    // it from another surface, a refusal (Denied) as well as a yes. Only an
    // operation already claimed or closed is a replay.
    let snapshot = store.snapshot(&request, now())?;
    anyhow::ensure!(
        matches!(
            snapshot.effect,
            EffectState::Awaiting | EffectState::Authorized | EffectState::Denied
        ),
        "this exact source call is already settled or claimed; its outcome must never be replayed"
    );
    if snapshot.resolution.is_some() {
        return Ok(Reviewed {
            answer: if sources::confirmed(&snapshot) == Some(true) {
                Answer::Allowed
            } else {
                Answer::Refused
            },
            store,
            request,
            operation,
        });
    }
    let envelope = store
        .plan_delivery(
            &request,
            "native:owner",
            "native",
            &native::capabilities(),
            None,
            now(),
        )
        .ok();
    let desktop = link_sandbox_route::desktop::only_desktop(desktop);
    let presence = store.active_routes(&format!("sandbox:{}", proposal.sandbox))?;
    let agent_present = proposal.agent.as_ref().is_some_and(|a| {
        store
            .active_routes(&format!("agent:{a}"))
            .is_ok_and(|r| !r.is_empty())
    });
    let cancelled = AtomicBool::new(false);
    let snapshot = if waits == Waits::IfAnswered && presence.is_empty() && !agent_present {
        match envelope.as_ref().and_then(|envelope| {
            dialog
                .response(&store, envelope, &desktop, wait, &cancelled)
                .map(|value| (envelope, value))
        }) {
            Some((envelope, value)) => {
                submit_value(&store, envelope, value)?;
            }
            None => {
                store.cancel(
                    &request,
                    &requester,
                    "no authorized owner answer was received",
                    now(),
                )?;
                return Ok(Reviewed {
                    answer: Answer::Unasked,
                    store,
                    request,
                    operation,
                });
            }
        }
        store.snapshot(&request, now())?
    } else {
        std::thread::scope(|scope| -> anyhow::Result<Snapshot> {
            let (store, envelope, desktop, cancelled) = (&store, &envelope, &desktop, &cancelled);
            scope.spawn(move || {
                if let Some(envelope) = envelope
                    && let Some(value) = dialog.response(store, envelope, desktop, wait, cancelled)
                {
                    let _ = submit_value(store, envelope, value);
                }
            });
            let result = store.wait(&request, || false);
            cancelled.store(true, Ordering::Relaxed);
            result
        })?
    };
    let answer = match snapshot.effect {
        EffectState::Authorized => Answer::Allowed,
        _ if snapshot.resolution.is_some() => Answer::Refused,
        _ => Answer::Unanswered,
    };
    Ok(Reviewed {
        answer,
        store,
        request,
        operation,
    })
}

fn submit(store: &Store, envelope: &DeliveryEnvelope, yes: bool) -> anyhow::Result<bool> {
    submit_value(
        store,
        envelope,
        ResponseValue::Submit(SetSubmission {
            answers: std::collections::BTreeMap::from([(
                envelope.question.questions[0].id.clone(),
                ItemAnswer::Confirmation { yes },
            )]),
            comment: None,
        }),
    )
}

fn submit_value(
    store: &Store,
    envelope: &DeliveryEnvelope,
    value: ResponseValue,
) -> anyhow::Result<bool> {
    let ingress = Ingress {
        actor: sources::owner(),
        route: envelope.delivery.route.clone(),
        surface: envelope.delivery.surface.clone(),
        input_kind: InputKind::Typed,
    };
    let response = ResponseCandidate {
        request: envelope.question.id.clone(),
        revision: envelope.question.revision,
        delivery: envelope.delivery.id.clone(),
        value,
        transport_event: TransportEventId::new(link_files::random_token()?)
            .map_err(anyhow::Error::msg)?,
    };
    Ok(store.submit(&ingress, response, now())? == SubmitDisposition::Accepted)
}

/// Owner-only command surface, displaying full review and a separate expiring code.
pub fn waiting(dir: &Path) -> anyhow::Result<Vec<DeliveryEnvelope>> {
    let store = store(dir)?;
    store
        .pending(now())?
        .into_iter()
        .filter(|s| s.question.requester.source.as_str() == "sandbox")
        .map(|s| {
            store.plan_delivery(
                &s.question.id,
                "terminal",
                "terminal",
                &render::text_capabilities(),
                None,
                now(),
            )
        })
        .collect()
}
pub fn answer(dir: &Path, code: &str, yes: bool) -> anyhow::Result<bool> {
    anyhow::ensure!(
        reply::is_code(code),
        "not a response code (lnk sandbox question list lists them)"
    );
    let store = store(dir)?;
    let Some(id) = store.find_code(code)? else {
        return Ok(false);
    };
    let snapshot = store.snapshot(&id, now())?;
    if snapshot.resolution.is_some() {
        return Ok(false);
    }
    anyhow::ensure!(
        snapshot.question.requester.source.as_str() == "sandbox",
        "not a sandbox question"
    );
    let envelope = store.plan_delivery(
        &id,
        "terminal",
        "terminal",
        &render::text_capabilities(),
        None,
        now(),
    )?;
    submit(&store, &envelope, yes)
}

#[expect(
    clippy::disallowed_methods,
    reason = "polls for owner questions outside the sandbox"
)]
pub fn answer_on_terminal(dir: PathBuf, sandbox: String) {
    let Some(mut tty) = link_format::streams::Terminal::open() else {
        return;
    };
    let Ok(store) = store(&dir) else {
        return;
    };
    let Ok(presence) = store.presence(&format!("sandbox:{sandbox}"), None) else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("answering".into())
        .spawn(move || {
            let _presence = presence;
            let mut shown = std::collections::BTreeSet::new();
            loop {
                for envelope in waiting(&dir).unwrap_or_default() {
                    let requester = &envelope.question.requester;
                    if requester.source.as_str() != "sandbox"
                        || requester.label != sandbox
                        || !shown.insert(envelope.question.id.clone())
                    {
                        continue;
                    }
                    let prompt = link_format::Question::code(
                        link_format::Sentence::adopt(&render::delivery(&envelope)),
                        &envelope.response_code,
                    );
                    if let Ok(Some(yes)) = prompt.ask_on_terminal(
                        &mut tty,
                        Duration::from_secs(envelope.question.expires_at.saturating_sub(now())),
                    ) {
                        let _ = submit(&store, &envelope, yes);
                    }
                }
                std::thread::sleep(POLL);
            }
        });
}

#[cfg(test)]
mod tests;
