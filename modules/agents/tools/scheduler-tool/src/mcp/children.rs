//! A message to a retained child (`message`), by its task's ID: each
//! refusal says what to do instead, as a model reads it.

use super::*;

pub(super) fn message(
    tool: &Tool,
    made: &Made,
    args: &Value,
    id: &str,
    hops: u32,
) -> anyhow::Result<Job> {
    let interactions = tool.store.interactions.as_ref().context(
        "Not sent: this harness keeps no retained children. Start a subagent with the task \
             instead.",
    )?;
    let token = made.meta["link.local/work"].as_str().with_context(|| {
        format!(
            "Not sent: only a conversation its harness runs as retained work may message a \
                 child. {LIST_CALL} shows your subagents; start a new one instead."
        )
    })?;
    let revision = made.meta["link.local/control_revision"].as_u64().context(
        "Not sent: the harness gave no control revision with the call, so the child can't be \
             checked. Try again on your next turn.",
    )?;
    let target = args["id"].as_str().with_context(|| {
        format!("Not sent: 'id' is a retained child's task ID, as {LIST_CALL} gives it.")
    })?;
    let thread = interactions.child_message_target(token, revision, target)?;
    Ok(Job {
        occurrence: None,
        portable: None,
        id: id.into(),
        kind: Kind::ChildMessage,
        thread,
        text: super::making::message(args, "message")?,
        answer: AnswerTo::Nowhere,
        due: now(),
        cron: None,
        hops,
        created: now(),
        tries: 0,
        tools: None,
        standing: made.standing()?,
        model: None,
        to: None,
        parent: super::making::coordinator(made),
        after: Vec::new(),
        description: None,
        outcome: None,
    })
}
