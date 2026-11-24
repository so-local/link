//! Explicit owner command grammar, shared by authenticated chat and CLI ingress.
use crate::{runtime::Binding, *};
use anyhow::{Context, ensure};
use link_interactions_contract::control::*;
use std::collections::BTreeSet;

pub fn is_command(text: &str) -> bool {
    text == "/stop"
        || text.starts_with("/work ")
        || text.starts_with("/chat:queue ")
        || text.starts_with("/chat:steer ")
}

/// Whether this is a bare `/stop` with no retained work on `thread`, which
/// isn't a refused control: the harness says what it stopped, if anything.
pub fn stops_nothing_retained(store: &Store, agent: &str, thread: &str, text: &str) -> bool {
    text == "/stop"
        && store
            .works(agent)
            .is_ok_and(|works| !works.iter().any(|b| b.state.work.session == thread))
}

/// The transport must establish ownership before calling this function.
pub fn command(
    store: &Store,
    actor: &Actor,
    agent: &str,
    thread: &str,
    text: &str,
    transport_event: TransportEventId,
) -> anyhow::Result<String> {
    ensure!(*actor == sources::owner(), "this is not the local owner");
    let text = if text == "/stop" {
        "stop"
    } else {
        text.strip_prefix("/work ")
            .or_else(|| text.strip_prefix("/chat:"))
            .context("unknown work command")?
    };
    let (verb, mut rest) = text.split_once(' ').unwrap_or((text, ""));
    if verb == "receipt" {
        return Ok(store
            .receipts(rest)?
            .iter()
            .map(|r| format!("{}: {:?} — {}", r.target.session, r.stage, r.evidence))
            .collect::<Vec<_>>()
            .join("\n"));
    }
    if verb == "batch" {
        let status = store.control_status(&ControlId::new(rest).map_err(anyhow::Error::msg)?)?;
        return Ok(format!(
            "{:?}: {}",
            status.stage,
            status
                .outcomes
                .iter()
                .map(|r| format!("{} {:?}", r.target.session, r.stage))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut scope = TreeScope::SelfOnly;
    let mut selected = None;
    loop {
        let (head, tail) = rest.split_once(' ').unwrap_or((rest, ""));
        match head {
            "--subtree" => scope = TreeScope::Subtree,
            "--descendants" => scope = TreeScope::Descendants,
            _ if head.starts_with("--target=") => selected = head.strip_prefix("--target="),
            _ => break,
        }
        rest = tail.trim_start();
    }
    let works = store.works(agent)?;
    if verb == "list" {
        return Ok(works.iter().map(status).collect::<Vec<_>>().join("\n"));
    }
    let selected: Vec<_> = if let Some(ids) = selected {
        ids.split(',').map(|id| works.iter().find(|b| b.state.work.attempt == id || b.state.work.task.as_deref() == Some(id)
            || matches!(&b.state.work.kind, WorkKind::Schedule {id: schedule, ..} if schedule.0 == id))
            .context("this harness has no retained work for that target")).collect::<anyhow::Result<_>>()?
    } else {
        vec![
            works
                .iter()
                .find(|b| b.state.work.session == thread)
                .context("this harness has no retained work for that target")?,
        ]
    };
    let work = *selected
        .first()
        .context("select at least one work target")?;
    if verb == "status" {
        return Ok(selected
            .iter()
            .map(|b| status(b))
            .collect::<Vec<_>>()
            .join("\n"));
    }
    let selector = if selected.len() > 1 {
        ensure!(
            scope == TreeScope::SelfOnly,
            "use one root for a recursive control"
        );
        ControlSelector::Selected {
            members: selected.iter().map(|b| b.state.work.clone()).collect(),
        }
    } else if scope == TreeScope::SelfOnly {
        ControlSelector::Exact {
            work: work.state.work.clone(),
        }
    } else {
        ControlSelector::Tree {
            root: work.state.work.clone(),
            scope: scope.clone(),
        }
    };
    let mut redirect = None;
    if matches!(verb, "queue" | "steer" | "redirect") {
        ensure!(
            selected.len() == 1 && scope == TreeScope::SelfOnly,
            "an input requires one exact conversation or child"
        );
        ensure!(!rest.trim().is_empty(), "input needs text");
        let id = InputId::new(link_files::random_token()?).map_err(anyhow::Error::msg)?;
        let input = UserInput {
            id: id.clone(),
            actor: actor.clone(),
            target: work.state.work.clone(),
            content: TextEvidence {
                text: rest.into(),
                supplied_by: actor.clone(),
                input_kind: InputKind::Typed,
            },
            admission: if verb == "steer" {
                Admission::Steer
            } else {
                Admission::Queue
            },
            transport_event: transport_event.clone(),
            submitted_at: now(),
        };
        let receipt = store.admit_input(input, &work.state, now())?;
        if verb != "redirect" {
            return Ok(format!(
                "{:?}: {}. Inclusion will be reported after the runtime uses it.",
                receipt.stage, id
            ));
        }
        redirect = Some(id);
    }
    let operation = match verb {
        "stop" => ControlOperation::Stop,
        "pause" => ControlOperation::Pause {
            boundary: match rest {
                "" => PauseBoundary::FinishModelResponse,
                "--immediate" => PauseBoundary::InterruptCurrentStep,
                _ => anyhow::bail!("pause takes only --immediate"),
            },
        },
        "resume" => {
            let targets = link_controls::select(actor, &selector, &store.lineage()?)?;
            ControlOperation::Resume {
                reasons: works
                    .iter()
                    .filter(|b| targets.contains(&b.state.work))
                    .flat_map(|b| b.state.pause_reasons.iter().cloned())
                    .collect(),
            }
        }
        "start" => ControlOperation::StartPreparedWork,
        "archive" => ControlOperation::Archive,
        "unarchive" => ControlOperation::Unarchive,
        "delete" => ControlOperation::Delete {
            scope: match rest {
                "conversation" => DeleteScope::Conversation,
                "task" => DeleteScope::TaskRecord,
                _ if rest.starts_with("artifacts ") => {
                    let ids: Vec<String> = rest[10..]
                        .split(',')
                        .filter(|id| !id.is_empty())
                        .map(str::to_owned)
                        .collect();
                    ensure!(!ids.is_empty(), "artifact deletion needs explicit IDs");
                    DeleteScope::Artifacts { ids }
                }
                _ => anyhow::bail!(
                    "choose the deletion scope: conversation, task, or artifacts with explicit IDs"
                ),
            },
        },
        "hold" => ControlOperation::AddHold {
            id: HoldId::new(link_files::random_token()?).map_err(anyhow::Error::msg)?,
            terms: HoldTerms {
                children: if rest.is_empty() {
                    None
                } else {
                    Some(rest.split(',').map(str::to_string).collect())
                },
                blocked_actions: BTreeSet::from([
                    DelegationAction::SpawnChild,
                    DelegationAction::StartChild,
                    DelegationAction::DispatchChildMessage,
                ]),
            },
        },
        "release" => ControlOperation::ReleaseHold {
            hold: HoldId::new(rest).map_err(anyhow::Error::msg)?,
        },
        "redirect" => ControlOperation::Redirect {
            input: redirect.context("missing redirect input")?,
        },
        _ => anyhow::bail!("unknown work command"),
    };
    let control = WorkControl {
        id: ControlId::new(link_files::random_token()?).map_err(anyhow::Error::msg)?,
        actor: actor.clone(),
        selector,
        operation,
        transport_event: TransportEventId::new(format!("control:{transport_event}"))
            .map_err(anyhow::Error::msg)?,
    };
    let batch = store.control_work(control, &store.lineage()?, now())?;
    if batch.members.is_empty() {
        return Ok(format!(
            "Batch {}: no matching work in the recorded scope.",
            batch.control.id
        ));
    }
    let mut notices = vec![format!("Batch {}", batch.control.id)];
    for member in &batch.members {
        let receipt = store
            .receipts(member.id.as_str())?
            .pop()
            .context("missing control receipt")?;
        let extra = match &member.operation {
            ControlOperation::AddHold { id, .. } => format!("; hold {id}"),
            _ => String::new(),
        };
        notices.push(format!(
            "{}: {:?} ({}{}).",
            member.work.session, receipt.stage, member.id, extra
        ));
    }
    Ok(notices.join("\n"))
}

fn status(b: &Binding) -> String {
    format!(
        "{} [{}]: {:?}, {:?}; pause reasons: {}. Progress is retained.",
        b.state.work.session,
        b.state.work.attempt,
        b.state.lifecycle,
        b.state.observed,
        b.state
            .pause_reasons
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )
}
