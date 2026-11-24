//! Correlation and state validation for one native conversation's
//! evidence. Consumers provide only exporter-authenticated native records.

use super::*;
use anyhow::{Context, bail, ensure};

/// An observation with the sequence assigned by its successful append.
#[derive(Clone, Copy)]
pub struct Located<'a> {
    pub seq: u64,
    pub evidence: &'a Evidence,
}

#[derive(Debug)]
pub struct Finished<'a> {
    pub seq: u64,
    pub offset_ns: Option<u64>,
    pub outcome: &'a Outcome,
}

/// A proposal and everything actually recorded about it. Missing
/// transitions remain missing, never synthesized as success or refusal.
#[derive(Debug)]
pub struct Execution<'a> {
    pub proposal: &'a Proposal,
    pub proposed: &'a ProposedCall,
    pub dispatched_ns: Option<u64>,
    pub dispatch: Option<&'a McpDispatch>,
    pub finished: Option<Finished<'a>>,
}

#[derive(Debug, Default)]
pub struct Index<'a> {
    pub requests: BTreeMap<u64, &'a request::ModelRequest>,
    /// Producing request -> native response event. Set only by
    /// `index_with_responses`, not inferred from model text.
    pub responses: BTreeMap<u64, u64>,
    pub executions: BTreeMap<CallRef, Execution<'a>>,
    pub skills: Vec<(u64, &'a SkillLoad)>,
}

/// Refuses duplicate, orphaned, out-of-order, or impossible transitions.
/// Record sequences may have gaps for ordinary conversation events.
pub fn index<'a>(records: impl IntoIterator<Item = Located<'a>>) -> anyhow::Result<Index<'a>> {
    let mut index = Index::default();
    let mut last = 0;
    // The model request before, and its every message, while known.
    let mut before: Option<(u64, Resolved)> = None;
    for Located { seq, evidence } in records {
        ensure!(seq > last, "execution evidence event {seq} is out of order");
        last = seq;
        match &evidence.record {
            Record::ModelRequested { request } => {
                request.validate()?;
                let projected = resolve(before.as_ref(), request)?;
                before = Some((seq, projected));
                for source in request
                    .projection
                    .retained()
                    .into_iter()
                    .flat_map(|projection| projection.own_sources())
                {
                    if let request::Source::Native { seq: source } = source.source {
                        ensure!(source < seq, "a model request names a future source");
                    }
                }
                index.requests.insert(seq, request);
            }
            Record::Proposed(proposal) => {
                let calls: Vec<_> = proposal
                    .calls
                    .iter()
                    .map(|proposed| proposed.call.clone())
                    .collect();
                link_model_contract::chat::calls::check_ids(&calls)?;
                for (ordinal, proposed) in proposal.calls.iter().enumerate() {
                    let call = CallRef {
                        proposal: seq,
                        ordinal: ordinal.try_into().context("too many proposed calls")?,
                    };
                    ensure!(
                        index
                            .executions
                            .insert(
                                call,
                                Execution {
                                    proposal,
                                    proposed,
                                    dispatched_ns: None,
                                    dispatch: None,
                                    finished: None,
                                }
                            )
                            .is_none(),
                        "execution {call:?} was proposed twice"
                    );
                }
            }
            Record::Dispatched {
                call,
                offset_ns,
                request,
            } => {
                request.arguments.validate()?;
                ensure!(
                    !request.server.is_empty() && !request.operation.is_empty(),
                    "a dispatch has no server or operation"
                );
                if let Some(arguments) = request.arguments.retained() {
                    ensure!(
                        arguments.is_object(),
                        "MCP dispatch arguments are not an object"
                    );
                }
                let execution = asked(&mut index, call)?;
                ensure!(
                    execution.finished.is_none(),
                    "execution {call:?} already finished"
                );
                execution.dispatch = Some(request);
                ensure!(
                    execution.dispatched_ns.replace(*offset_ns).is_none(),
                    "execution {call:?} was dispatched twice"
                );
            }
            Record::Finished {
                call,
                offset_ns,
                outcome,
            } => {
                if let Outcome::Completed { reply } = outcome {
                    reply.validate()?;
                }
                let execution = asked(&mut index, call)?;
                ensure!(
                    execution.finished.is_none(),
                    "execution {call:?} finished twice"
                );
                match (execution.dispatched_ns, outcome) {
                    (None, Outcome::Completed { .. } | Outcome::RemoteError { .. }) => {
                        bail!("execution {call:?} has a reply without a dispatch marker");
                    }
                    (Some(_), Outcome::Refused { .. }) => {
                        bail!("execution {call:?} was refused after possible dispatch");
                    }
                    _ => {}
                }
                if let (Some(started), Some(ended)) = (execution.dispatched_ns, offset_ns) {
                    ensure!(
                        *ended >= started,
                        "execution {call:?} ended before dispatch"
                    );
                }
                execution.finished = Some(Finished {
                    seq,
                    offset_ns: *offset_ns,
                    outcome,
                });
            }
            Record::SkillLoaded { load } => {
                load.validate()?;
                if let SkillAnchor::Call { call } = load.anchor {
                    let execution = asked(&mut index, &call)?;
                    ensure!(
                        execution.finished.is_some(),
                        "skill load precedes execution {call:?}'s result"
                    );
                }
                index.skills.push((seq, load));
            }
        }
    }
    Ok(index)
}

/// Include the trusted native event-to-request links supplied by the
/// exporter. A prepared attempt with no response cannot prove model use.
pub fn index_with_responses<'a>(
    records: impl IntoIterator<Item = Located<'a>>,
    responses: impl IntoIterator<Item = (u64, u64)>,
) -> anyhow::Result<Index<'a>> {
    let mut index = index(records)?;
    for (response, request) in responses {
        ensure!(
            request < response && index.requests.contains_key(&request),
            "a native response names no prior model request"
        );
        ensure!(
            index.responses.insert(request, response).is_none(),
            "a model attempt has two native responses"
        );
    }
    Ok(index)
}

impl Index<'_> {
    pub fn producing_request(&self, call: CallRef) -> Option<(u64, &request::ModelRequest)> {
        let execution = self.executions.get(&call)?;
        self.responses.iter().find_map(|(request, response)| {
            if *response != call.proposal {
                return None;
            }
            let model = self.requests.get(request)?;
            (model.phase == execution.proposal.phase).then_some((*request, *model))
        })
    }
    /// Exact source inclusion in an answered canonical text request.
    /// Loading, source authority, and model inclusion must all hold.
    pub fn supplied_requests(&self, event: u64, load: &SkillLoad) -> Vec<u64> {
        if !load.complete
            || !matches!(
                load.authority,
                ReadAuthority::Managed { .. } | ReadAuthority::Runtime
            )
        {
            return vec![];
        }
        let origin = match load.anchor {
            SkillAnchor::Owner => event,
            SkillAnchor::Call { call } => match self
                .executions
                .get(&call)
                .and_then(|call| call.finished.as_ref())
            {
                Some(finished) => finished.seq,
                None => return vec![],
            },
        };
        let mut supplied = Vec::new();
        let mut before: Option<(u64, Resolved)> = None;
        for (seq, request) in &self.requests {
            let resolved = resolve(before.as_ref(), request).unwrap_or(Resolved::Unknown);
            let projected = match &resolved {
                Resolved::Known(projected) => Some(projected),
                Resolved::Omitted | Resolved::Unknown => None,
            };
            if self.responses.contains_key(seq)
                && projected.iter().flat_map(|p| &p.sources).any(|source| {
                    source.source == request::Source::Native { seq: origin }
                        && source.coverage == request::Coverage::Whole
                        && source.source_text == load.model_view
                })
            {
                supplied.push(*seq);
            }
            before = Some((*seq, resolved));
        }
        supplied
    }

    /// Every message the model request numbered `seq` sent, and their
    /// sources, made from the requests it goes on from: none when it or
    /// one of those was omitted, or isn't here.
    pub fn projection(&self, seq: u64) -> Option<request::Projected> {
        let mut chain = Vec::new();
        let mut at = seq;
        let whole = loop {
            match self.requests.get(&at)?.projection.retained()? {
                request::RequestProjection::Whole(whole) => break whole.clone(),
                request::RequestProjection::Continued(continued) => {
                    chain.push(continued);
                    at = continued.after;
                }
            }
        };
        chain
            .into_iter()
            .rev()
            .try_fold(whole, |projected, continued| {
                projected.continued(continued).ok()
            })
    }
}

/// What is known of a model request's every message.
enum Resolved {
    Known(request::Projected),
    /// Its projection was omitted.
    Omitted,
    /// It goes on from a request the records don't hold: a snapshot
    /// missing its start.
    Unknown,
}

/// What is known of every message `request` sent and their sources,
/// `before` being the model request before it in the conversation: an
/// error when it names another request than that one, or goes on from
/// one omitted.
fn resolve(
    before: Option<&(u64, Resolved)>,
    request: &request::ModelRequest,
) -> anyhow::Result<Resolved> {
    let Some(projection) = request.projection.retained() else {
        return Ok(Resolved::Omitted);
    };
    match (projection, before) {
        (request::RequestProjection::Whole(whole), _) => Ok(Resolved::Known(whole.clone())),
        (request::RequestProjection::Continued(_), None) => Ok(Resolved::Unknown),
        (request::RequestProjection::Continued(continued), Some((seq, resolved))) => {
            ensure!(
                continued.after == *seq,
                "a continued model request names {} but follows {seq}",
                continued.after
            );
            match resolved {
                Resolved::Known(projected) => Ok(Resolved::Known(projected.continued(continued)?)),
                Resolved::Omitted => bail!("a continued model request goes on from an omitted one"),
                Resolved::Unknown => Ok(Resolved::Unknown),
            }
        }
    }
}

fn asked<'a, 'b>(
    index: &'b mut Index<'a>,
    call: &CallRef,
) -> anyhow::Result<&'b mut Execution<'a>> {
    index
        .executions
        .get_mut(call)
        .with_context(|| format!("execution {call:?} wasn't proposed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal() -> Evidence {
        Evidence::new(Record::Proposed(Proposal {
            phase: Phase::Turn,
            attempt: "attempt".into(),
            work: None,
            started_unix_ms: 0,
            calls: vec![ProposedCall {
                provider_id: "repeated-provider-id".into(),
                call: Call {
                    id: "repeated-provider-id".into(),
                    ..Default::default()
                },
            }],
        }))
    }

    fn located(seq: u64, evidence: &Evidence) -> Located<'_> {
        Located { seq, evidence }
    }

    #[test]
    fn repeated_provider_ids_are_scoped_to_proposal_and_missing_results_stay_missing() {
        let (one, two) = (proposal(), proposal());
        let trace = index([located(4, &one), located(9, &two)]).unwrap();
        assert_eq!(trace.executions.len(), 2);
        assert!(
            trace
                .executions
                .values()
                .all(|call| call.finished.is_none())
        );
    }

    #[test]
    fn duplicate_normalized_ids_in_one_proposal_are_refused() {
        let mut bad = proposal();
        let Record::Proposed(proposed) = &mut bad.record else {
            unreachable!()
        };
        proposed.calls.push(proposed.calls[0].clone());
        assert!(index([located(4, &bad)]).is_err());
    }

    #[test]
    fn unmatched_duplicate_and_impossible_transitions_are_refused() {
        let proposed = proposal();
        let call = CallRef {
            proposal: 4,
            ordinal: 0,
        };
        let dispatched = Evidence::new(Record::Dispatched {
            call,
            offset_ns: 12,
            request: McpDispatch {
                server: "fixture".into(),
                operation: "read".into(),
                arguments: Captured::Retained {
                    value: serde_json::json!({}),
                },
            },
        });
        let refused = Evidence::new(Record::Finished {
            call,
            offset_ns: Some(20),
            outcome: Outcome::Refused {
                reason: "not offered".into(),
            },
        });
        assert!(index([located(5, &dispatched)]).is_err());
        assert!(
            index([
                located(4, &proposed),
                located(5, &dispatched),
                located(6, &dispatched)
            ])
            .is_err()
        );
        assert!(
            index([
                located(4, &proposed),
                located(5, &dispatched),
                located(6, &refused)
            ])
            .is_err()
        );
        assert!(index([located(4, &proposed), located(5, &refused)]).is_ok());
        assert!(
            index([
                located(4, &proposed),
                located(5, &refused),
                located(6, &refused)
            ])
            .is_err()
        );
        let unknown = Evidence::new(Record::Finished {
            call,
            offset_ns: Some(11),
            outcome: Outcome::Unknown {
                reason: "lost connection".into(),
            },
        });
        assert!(
            index([
                located(4, &proposed),
                located(5, &dispatched),
                located(6, &unknown)
            ])
            .is_err()
        );
    }
}
