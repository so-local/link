//! The source review gate retains exact calls and owner resolutions.
use super::{
    Tool, check_call,
    route::{Tools, work_of},
    session::Session,
};
use serde_json::Value;

pub(super) async fn gate(
    tools: &Tools,
    tool: &Tool,
    session: &Session,
    name: &str,
    message: &Value,
) -> Result<Option<crate::bridges::asks::Reviewed>, String> {
    check_call(name).map_err(|e| e.to_string())?;
    let arguments = message.pointer("/params/arguments").unwrap_or(&Value::Null);
    if !tool.asks_given_by(name, arguments, tools.by) {
        return Ok(None);
    }
    let question = question(
        &tools.sandbox,
        &tool.name,
        name,
        match name {
            "resources/read" | "prompts/get" => message.pointer("/params"),
            _ => message.pointer("/params/arguments"),
        }
        .unwrap_or(&Value::Null),
        &session.hints_of(name),
    );
    let dir = tools.dir.clone();
    let allow = tool.lets_through(name);
    let desktop = tools.desktop.clone();
    let asking = tools.asking.as_ref();
    let mut question = question.for_agent(asking.map(|a| a.agent.as_str()));
    question.work = work_of(tools, message)?;
    question.call = message["params"]["_meta"]["link.local/call"]
        .as_str()
        .map(str::to_owned);
    question.payload = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    let wait = crate::bridges::asks::wait_of(asking);
    // Nobody here to ask: refused at once, rather than at `ask_wait`.
    let waits = crate::bridges::asks::Waits::IfAnswered;
    let asked = move || crate::bridges::asks::ask(&dir, &question, &desktop, waits, wait);
    match tokio::task::spawn_blocking(asked).await {
        Ok(Ok(reviewed)) if reviewed.answer == crate::bridges::asks::Answer::Allowed => {
            Ok(Some(reviewed))
        }
        Ok(Ok(reviewed)) if reviewed.answer == crate::bridges::asks::Answer::Refused => {
            Err(reviewed.refusal())
        }
        Ok(Ok(reviewed)) if reviewed.answer == crate::bridges::asks::Answer::Unanswered => {
            Err(format!(
                "{}'s {name} asks you first, and no one answered in {} seconds \
             (lnk sandbox question list shows what's waiting; {allow})",
                tool.name,
                wait.as_secs()
            ))
        }
        Ok(Ok(_)) => Err(format!(
            "{}'s {name} asks you first, and no authorized owner answer was received: refused ({allow})",
            tool.name
        )),
        Ok(Err(err)) => Err(format!(
            "{}'s {name} asks you first, and asking failed: {err:#}",
            tool.name
        )),
        Err(_) => Err("asking you failed".into()),
    }
}

/// What a call asks you: the call, its arguments, and what the server
/// says of it, as its own word.
pub fn question(
    sandbox: &str,
    tool: &str,
    call: &str,
    arguments: &Value,
    hints: &Value,
) -> crate::bridges::asks::Proposal {
    let hint = |k: &str| hints.get(k).and_then(Value::as_bool);
    let mut details = vec![format!("With {arguments}")];
    match (hint("readOnlyHint"), hint("destructiveHint")) {
        (Some(true), _) => details.push("It says it only reads (its own word, not Link's).".into()),
        (_, Some(true)) => details.push("It says it may delete or change things.".into()),
        _ => {}
    }
    crate::bridges::asks::Proposal::sandbox(sandbox, format!("run tool {tool}'s {call}"), details)
}
