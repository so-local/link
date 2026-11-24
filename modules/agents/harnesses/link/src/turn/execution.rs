//! One recording path for ordinary and summary tool requests.

use super::*;
use link_harness_wire::evidence::{CallRef, Evidence, Phase, Proposal, ProposedCall, Record};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(super) struct Clock {
    started: std::time::Instant,
    unix_ms: u64,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            started: std::time::Instant::now(),
            unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis().min(u64::MAX as u128) as u64),
        }
    }

    fn offset(self) -> u64 {
        self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64
    }

    pub fn proposal(
        self,
        turn: &Turn<'_>,
        answer: &Event,
        provider_ids: Vec<String>,
        phase: Phase,
        work: Option<link_interactions::WorkRef>,
    ) -> Evidence {
        Evidence::new(Record::Proposed(Proposal {
            phase,
            attempt: turn.attempt.into(),
            work,
            started_unix_ms: self.unix_ms,
            calls: answer
                .calls
                .iter()
                .cloned()
                .zip(provider_ids)
                .map(|(call, provider_id)| ProposedCall { provider_id, call })
                .collect(),
        }))
    }
}

pub(super) enum Policy {
    Turn { limited: bool },
    Summary { save: bool },
}

impl Policy {
    fn refusal(&self, call: &event::Call) -> Option<&'static str> {
        match self {
            Self::Turn { limited: true } => Some(TOO_MANY),
            Self::Summary { save: false } => Some(compact::NOT_WHILE_SUMMARIZING),
            Self::Summary { save: true } if call.name != compact::REMEMBER => {
                Some(compact::NOT_WHILE_SAVING)
            }
            _ => None,
        }
    }
}

pub(super) struct Batch<'a> {
    pub origin: u64,
    pub clock: Clock,
    pub phase: Phase,
    pub calls: &'a [event::Call],
    pub expected: &'a Option<link_interactions::WorkRef>,
    pub meta: serde_json::Value,
    pub policy: Policy,
}

impl<F> crate::tools::Observer for F
where
    F: Fn(crate::tools::Dispatch<'_>) -> anyhow::Result<()> + Send + Sync,
{
    fn dispatching(&self, request: crate::tools::Dispatch<'_>) -> anyhow::Result<()> {
        self(request)
    }
}

struct CallObserver<D, F> {
    dispatch: D,
    finish: F,
    result: std::sync::Mutex<Option<Event>>,
}

impl<D, F> crate::tools::Observer for CallObserver<D, F>
where
    D: Fn(crate::tools::Dispatch<'_>) -> anyhow::Result<()> + Send + Sync,
    F: Fn(&mut Answered) -> anyhow::Result<Event> + Send + Sync,
{
    fn dispatching(&self, request: crate::tools::Dispatch<'_>) -> anyhow::Result<()> {
        (self.dispatch)(request)
    }
    fn received(&self, answer: &mut Answered) -> anyhow::Result<()> {
        let mut result = self
            .result
            .lock()
            .map_err(|_| anyhow::anyhow!("tool receipt lock poisoned"))?;
        if result.is_none() {
            *result = Some((self.finish)(answer)?);
        }
        Ok(())
    }
}

impl Harness {
    pub(super) async fn execute<'b>(
        &self,
        turn: &Turn<'_>,
        toolbox: &Toolbox<'_>,
        batch: Batch<'b>,
        events: &mut Vec<Event>,
    ) -> anyhow::Result<Vec<Event>> {
        let events = std::sync::Mutex::new(events);
        let record = |mut event: Event| -> anyhow::Result<Event> {
            let mut held = events
                .lock()
                .map_err(|_| anyhow::anyhow!("execution recorder lock poisoned"))?;
            off_runtime(|| self.append_next(turn.agent, turn.thread, &mut event, &mut held))?;
            turn.wrote(&event);
            Ok(event)
        };
        let run = |ordinal: usize, call: &'b event::Call| {
            let (record, batch) = (&record, &batch);
            async move {
                let reference = CallRef {
                    proposal: batch.origin,
                    ordinal: ordinal.try_into()?,
                };
                let started = std::time::Instant::now();
                let observer = CallObserver {
                    dispatch: |request: crate::tools::Dispatch<'_>| {
                        self.same_step(turn, batch.expected)?;
                        record(Event {
                            role: event::EVIDENCE.into(),
                            at: event::now(),
                            evidence: Some(
                                Evidence::new(Record::Dispatched {
                                    call: reference,
                                    offset_ns: batch.clock.offset(),
                                    request: crate::tools::dispatch(request),
                                })
                                .into(),
                            ),
                            ..Event::default()
                        })?;
                        Ok(())
                    },
                    finish: |answered: &mut Answered| {
                        let ended = batch.clock.offset();
                        let latency_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                        let (mut parts, mut media, mut left) =
                            (Vec::new(), BTreeMap::new(), Vec::new());
                        for (index, image) in answered
                            .media_sources
                            .iter()
                            .copied()
                            .zip(std::mem::take(&mut answered.media))
                        {
                            let (kept, omission) = self.keep(turn.agent, turn.thread, vec![image]);
                            match kept.into_iter().next() {
                                Some(part) => {
                                    media.insert(index, Ok(part.clone()));
                                    parts.push(part);
                                }
                                None => {
                                    let why =
                                        omission.unwrap_or_else(|| "media was not stored".into());
                                    left.push(why.clone());
                                    media.insert(index, Err(why));
                                }
                            }
                        }
                        let text = if left.is_empty() {
                            answered.text.clone()
                        } else {
                            format!("{}\n{}", answered.text, left.join("\n"))
                        };
                        let mut outcome = crate::tools::captured(answered, &text, &parts, &media);
                        let skill = match crate::tools::loaded(answered, reference, &text) {
                            Ok(load) => load,
                            Err(_) => {
                                if let link_harness_wire::evidence::Outcome::Completed { reply } =
                                    &mut outcome
                                {
                                    reply
                                        .model
                                        .omissions
                                        .push("skill read receipt could not be verified".into());
                                }
                                None
                            }
                        };
                        let result = record(Event {
                            role: if batch.phase == Phase::Turn {
                                event::TOOL
                            } else {
                                event::EVIDENCE
                            }
                            .into(),
                            text,
                            call: Some(call.id.clone()),
                            tool: Some(call.name.clone()),
                            failed: answered.failed,
                            at: event::now(),
                            latency_ms: Some(latency_ms),
                            parts,
                            evidence: Some(
                                Evidence::new(Record::Finished {
                                    call: reference,
                                    offset_ns: Some(ended),
                                    outcome,
                                })
                                .into(),
                            ),
                            ..Event::default()
                        })?;
                        if let Some(load) = skill {
                            record(Event {
                                role: event::EVIDENCE.into(),
                                at: event::now(),
                                evidence: Some(
                                    Evidence::new(Record::SkillLoaded {
                                        load: Box::new(load),
                                    })
                                    .into(),
                                ),
                                ..Event::default()
                            })?;
                        }
                        Ok::<Event, anyhow::Error>(result)
                    },
                    result: std::sync::Mutex::new(None),
                };
                let mut answered = match batch.policy.refusal(call) {
                    Some(why) => Answered::failed(why),
                    None if self.same_step(turn, batch.expected).is_err() => {
                        Answered::failed("Work paused or stopped before this call was dispatched.")
                    }
                    None => {
                        toolbox
                            .call_observed(call, turn.cancelled.clone(), &batch.meta, &observer)
                            .await?
                    }
                };

                crate::tools::Observer::received(&observer, &mut answered)?;
                observer
                    .result
                    .into_inner()
                    .map_err(|_| anyhow::anyhow!("tool receipt lock poisoned"))?
                    .ok_or_else(|| anyhow::anyhow!("tool result was not recorded"))
            }
        };
        // A step's calls in order: those that only read, side by side, run
        // at once, and one that changes something runs alone, after every
        // call before it has ended and before any after it starts. The
        // results stay in the order the model asked for them.
        let calls = batch.calls;
        let mut made = Vec::with_capacity(calls.len());
        for step in steps(calls, |call| toolbox.reads_only(&call.name)) {
            let ran = futures_util::future::join_all(step.map(|n| run(n, &calls[n]))).await;
            for result in ran {
                made.push(result?);
            }
        }
        Ok(made)
    }
}

/// `calls` cut into the runs that go at once, by index: each run of calls
/// that only read (`reads`) is one, and each other call is one of its own.
fn steps(
    calls: &[event::Call],
    reads: impl Fn(&event::Call) -> bool,
) -> Vec<std::ops::Range<usize>> {
    let mut steps: Vec<std::ops::Range<usize>> = Vec::new();
    for (n, call) in calls.iter().enumerate() {
        let read = reads(call);
        match steps.last_mut() {
            Some(last) if read && last.end == n && reads(&calls[last.start]) => last.end = n + 1,
            _ => steps.push(n..n + 1),
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str) -> event::Call {
        event::Call {
            id: name.into(),
            name: name.into(),
            arguments: serde_json::json!({}),
        }
    }

    /// Reads side by side share a run; each change has one of its own,
    /// and a read after a change starts a new run.
    #[test]
    fn a_change_runs_alone_and_reads_run_together() {
        let calls: Vec<_> = ["list", "read", "delete", "list", "stop", "delete", "read"]
            .into_iter()
            .map(call)
            .collect();
        let reads = |call: &event::Call| call.name == "list" || call.name == "read";
        assert_eq!(steps(&calls, reads), [0..2, 2..3, 3..4, 4..5, 5..6, 6..7]);
        assert_eq!(
            steps(&calls[..2], reads),
            vec![std::ops::Range { start: 0, end: 2 }]
        );
        assert!(steps(&[], reads).is_empty());
    }
}
