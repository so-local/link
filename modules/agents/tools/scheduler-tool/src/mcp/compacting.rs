//! The `compact` call: the harness keeping its conversation's idle
//! compaction ([`Kind::Compact`]), due so long from now. Never offered
//! (`tools/list`): the model neither sees nor calls it.

use super::*;
use link_scheduler_tool_contract::compaction;

/// Keeps the compaction job of the conversation `made` was called in, as
/// `arguments` say: due so long from `at`, or, with no time, none.
pub(super) fn keep(tool: &Tool, made: &Made, arguments: &Value, at: u64) -> anyhow::Result<Value> {
    let names = compaction::compaction();
    if arguments.get(&names.after).is_none_or(Value::is_null) {
        let thread = made
            .thread
            .as_deref()
            .context("compact needs the conversation it's called in")?;
        let id = compaction::job(thread);
        return Ok(json!({"id": id, "cancelled": tool.store.cancel(&id)?}));
    }
    let job = job(made, arguments, at)?;
    tool.store.put(&job)?;
    Ok(json!({"id": job.id, "kind": job.kind, "thread": job.thread,
        "next": link_time(job.due)}))
}

/// The compaction job of the conversation `made` was called in, due
/// `after` seconds from `at`, as `arguments` say: its ID the
/// conversation's own, so keeping it again moves the one waiting.
pub(super) fn job(made: &Made, arguments: &Value, at: u64) -> anyhow::Result<Job> {
    let thread = made
        .thread
        .clone()
        .context("compact needs the conversation it's called in")?;
    let names = compaction::compaction();
    let after = arguments
        .get(&names.after)
        .and_then(Value::as_u64)
        .filter(|after| *after > 0)
        .with_context(|| format!("'{}' is how many seconds from now, at least 1", names.after))?;
    Ok(Job {
        occurrence: None,
        portable: None,
        id: compaction::job(&thread),
        kind: Kind::Compact,
        text: names.text.clone(),
        answer: AnswerTo::Nowhere,
        due: at.saturating_add(after),
        cron: None,
        hops: 0,
        created: at,
        tries: 0,
        tools: None,
        model: None,
        to: None,
        parent: None,
        after: Vec::new(),
        description: None,
        outcome: None,
        standing: made.standing()?,
        thread,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made(thread: Option<&str>) -> Made {
        super::super::made(
            &json!({"link.local/thread": thread}),
            link_connection_contract::access::Standing::Owner,
        )
    }

    /// A conversation's compaction is due so long after the call, one
    /// job a conversation, its answer nowhere; a call with no
    /// conversation, or no time, is refused.
    #[test]
    fn a_conversations_compaction_is_due_after_its_quiet() {
        let job = job(&made(Some("telegram:42")), &json!({"after": 3300}), 1000).unwrap();
        assert_eq!(job.kind, Kind::Compact);
        assert_eq!(job.due, 4300);
        assert_eq!(job.id, compaction::job("telegram:42"));
        assert_eq!(job.answer, AnswerTo::Nowhere);
        assert_eq!(job.from(), compaction::from("telegram:42"));
        assert!(super::job(&made(None), &json!({"after": 3300}), 1000).is_err());
        for after in [json!(0), json!("55m")] {
            assert!(
                super::job(&made(Some("telegram:42")), &json!({"after": after}), 1000).is_err(),
                "{after}"
            );
        }
    }
}
