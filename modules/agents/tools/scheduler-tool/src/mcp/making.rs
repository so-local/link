//! The jobs a call makes: a subagent (`task`), a schedule, the tools
//! each gets, and their message.

use super::*;

/// A subagent: a new conversation, now, its answer back here.
pub(super) fn task(
    tool: &Tool,
    made: &Made,
    arguments: &Value,
    id: &str,
    hops: u32,
) -> anyhow::Result<Job> {
    let parent = made
        .thread
        .clone()
        .context("a subagent's answer needs a conversation to come back to")?;
    ensure!(
        tool.store.subagents() < tool.caps.subagents,
        "Not started: {} subagents at work, the cap ([tool.scheduler] subagents). Wait for \
         their answers.",
        tool.caps.subagents
    );
    let text = message(arguments, "prompt")?;
    let description = arguments
        .get("description")
        .map(|given| {
            given
                .as_str()
                .map(str::trim)
                .filter(|d| !d.is_empty() && d.len() <= limits().description_max)
                .map(str::to_string)
                .with_context(|| {
                    format!(
                        "'description' is a few words, at most {} bytes",
                        limits().description_max
                    )
                })
        })
        .transpose()?;
    let tools = tools_of(made, arguments)?;
    let model = match arguments.get("model") {
        None => made.model.clone(),
        Some(model) => Some(
            model
                .as_str()
                .filter(|m| link_connection_contract::is_model_name(m))
                .context("'model' is a model's name, or an alias")?
                .to_string(),
        ),
    };
    Ok(Job {
        occurrence: None,
        portable: None,
        id: id.to_string(),
        kind: Kind::Task,
        thread: format!("task-{id}"),
        text,
        answer: AnswerTo::Thread(parent),
        due: now(),
        cron: None,
        hops,
        created: now(),
        tries: 0,
        tools,
        standing: made.standing()?,
        model,
        to: None,
        parent: coordinator(made),
        after: after(tool, made, arguments)?,
        description,
        outcome: None,
    })
}

/// The task a call's conversation is, as its coordinator: the ID of
/// `task-<id>`; none for the agent's own conversations. Read from where
/// the call was made, never from what the model says.
pub(super) fn coordinator(made: &Made) -> Option<String> {
    made.thread
        .as_deref()
        .and_then(|thread| thread.strip_prefix("task-"))
        .filter(|id| link_machine::machine::check_id(id).is_ok())
        .map(str::to_string)
}

/// The jobs a subagent waits on (`after`): each one of this
/// conversation's subagents or messages, still to end or ended, at most
/// [`Limits::after_max`](crate::limits::Limits::after_max) of them.
fn after(tool: &Tool, made: &Made, arguments: &Value) -> anyhow::Result<Vec<String>> {
    let Some(asked) = arguments.get("after") else {
        return Ok(Vec::new());
    };
    let asked: Vec<String> = asked
        .as_array()
        .context("'after' is a list of jobs' IDs")?
        .iter()
        .map(|t| t.as_str().map(str::to_string))
        .collect::<Option<_>>()
        .context("'after' is a list of jobs' IDs")?;
    ensure!(
        asked.len() <= limits().after_max,
        "Not started: it may wait on {} jobs at most.",
        limits().after_max
    );
    let mine = super::mine(&tool.store, made)?;
    let mut after: Vec<String> = Vec::new();
    for id in asked {
        ensure!(
            mine.iter().any(|job| job.id == id && job.keeps_outcome()),
            "Not started: {} isn't a subagent or message of this conversation's: {} lists \
             them.",
            link_format::clean(&id),
            super::LIST_CALL
        );
        if !after.contains(&id) {
            after.push(id);
        }
    }
    Ok(after)
}

/// A subagent's tools: those `tools` names, each one of its parent's, or,
/// left out, all its parent's. None from a harness that doesn't say its
/// conversation's tools: it can't give a subagent fewer, so a list is
/// refused there rather than ignored.
fn tools_of(made: &Made, arguments: &Value) -> anyhow::Result<Option<Vec<String>>> {
    let Some(asked) = arguments.get("tools") else {
        return Ok(made.tools.clone());
    };
    let asked: Vec<String> = asked
        .as_array()
        .context("'tools' is a list of your tools' names")?
        .iter()
        .map(|t| t.as_str().map(str::to_string))
        .collect::<Option<_>>()
        .context("'tools' is a list of your tools' names")?;
    let Some(mine) = &made.tools else {
        bail!(
            "Not started: this harness can't give a subagent fewer tools. Leave out 'tools', and \
             it gets all of yours."
        );
    };
    let not_mine: Vec<&str> = asked
        .iter()
        .filter(|t| !mine.contains(t))
        .map(String::as_str)
        .collect();
    ensure!(
        not_mine.is_empty(),
        "Not started: {} isn't among your tools, which are: {}. A subagent gets yours, or fewer.",
        not_mine.join(", "),
        mine.join(", ")
    );
    let mut tools: Vec<String> = Vec::new();
    for tool in asked {
        if !tools.contains(&tool) {
            tools.push(tool);
        }
    }
    Ok(Some(tools))
}

/// A message to a conversation later, or every so often.
pub(super) fn schedule(
    tool: &Tool,
    made: &Made,
    arguments: &Value,
    id: &str,
    hops: u32,
) -> anyhow::Result<Job> {
    let text = message(arguments, "prompt")?;
    let thread = made
        .thread
        .clone()
        .context("a schedule is made in a conversation, the main chat unless the call names one")?;
    let expression = arguments
        .get("cron")
        .and_then(Value::as_str)
        .context("'cron' is when it fires: a standard 5-field cron expression")?;
    // Recurring unless it says not: fired once, it keeps no cron.
    let recurring = arguments
        .get("recurring")
        .map(|given| given.as_bool().context("'recurring' is true or false"))
        .transpose()?
        .unwrap_or(true);
    let cron = Cron::parse(expression)?;
    if recurring && let Some(interval) = tool.caps.interval {
        let gap = u64::from(cron.shortest_gap()) * 60;
        ensure!(
            gap >= interval.as_secs(),
            "Not scheduled: it would fire {} apart, more often than every {}, the \
                     shortest a schedule may repeat ([tool.scheduler] interval).",
            minutes(gap / 60),
            minutes(interval.as_secs().div_ceil(60))
        );
    }
    let due = cron
        .next_after(now())
        .context("that cron expression never fires")?;
    let answer = match arguments.get("answer") {
        None => here(tool, made, &thread)?,
        Some(given) => {
            let given = given
                .as_str()
                .context("'answer' is where your answer goes, by name")?;
            let answers = tool.answers();
            ensure!(
                answers.iter().any(|a| a == given),
                "Not scheduled: {} isn't where an answer may go, which is one of: {}.",
                link_format::clean(given),
                answers.join(", ")
            );
            match given {
                super::HERE => here(tool, made, &thread)?,
                super::NOWHERE => AnswerTo::Nowhere,
                connection => AnswerTo::Connection(connection.into()),
            }
        }
    };
    Ok(Job {
        occurrence: None,
        portable: None,
        id: id.to_string(),
        kind: Kind::Schedule,
        thread,
        text,
        answer,
        due,
        cron: recurring.then(|| expression.to_string()),
        hops,
        created: now(),
        tries: 0,
        // A subagent's conversation's tools, so a schedule it makes fires
        // with no more than it has. Another conversation's aren't kept: a
        // list narrows the conversation it fires in for good.
        tools: coordinator(made).and(made.tools.clone()),
        standing: made.standing()?,
        model: None,
        to: None,
        // A subagent's schedule goes with it: cancelling the subagent
        // cancels it.
        parent: coordinator(made),
        after: Vec::new(),
        description: None,
        outcome: None,
    })
}

/// `n` minutes, as a person writes them.
fn minutes(n: u64) -> String {
    match n {
        1 => "1 minute".into(),
        n => format!("{n} minutes"),
    }
}

/// Where `here` is for a schedule made in `thread`: its chat, for a
/// conversation with one (`telegram:42`, `link:main`); for a subagent's,
/// the conversation that started it, its answer coming back there as
/// the subagent's does. Refused for one with neither (a teammate's, a
/// schedule's own), whose answer couldn't be delivered when it fires.
fn here(tool: &Tool, made: &Made, thread: &str) -> anyhow::Result<AnswerTo> {
    if thread.contains(':') {
        return Ok(AnswerTo::Chat);
    }
    if let Some(id) = coordinator(made) {
        let started = tool
            .store
            .jobs()
            .into_iter()
            .chain(tool.store.firing())
            .chain(tool.store.ended_jobs())
            .find(|job| job.id == id && job.kind == Kind::Task);
        if let Some(Job {
            answer: AnswerTo::Thread(parent),
            ..
        }) = started
        {
            return Ok(AnswerTo::Thread(parent));
        }
    }
    bail!(
        "Not scheduled: this conversation has no chat for 'here' to answer in. Give 'answer': \
         nowhere, cli, or a connection."
    )
}

/// The message of `key`: some text, at most
/// [`Limits::text_max`](crate::limits::Limits::text_max) bytes.
pub(super) fn message(arguments: &Value, key: &str) -> anyhow::Result<String> {
    let text = arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .with_context(|| format!("'{key}' is the message"))?;
    let most = limits().text_max;
    ensure!(text.len() <= most, "'{key}' is over {most} bytes");
    Ok(text.to_string())
}

impl super::Made {
    /// The standing a job made from the call keeps: the conversation's,
    /// never above the agent's floor. A claim that isn't a standing is
    /// refused.
    pub(super) fn standing(&self) -> anyhow::Result<link_connection_contract::access::Standing> {
        self.standing.context(
            "this call's standing (link.local/standing) is owner, member or stranger, or left out",
        )
    }
}
