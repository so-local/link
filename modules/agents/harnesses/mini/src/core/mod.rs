//! The harness loop. Model, tool and recording operations are supplied by its caller.

pub use link_model_contract::chat::{Answer, Answered, Call, Message};

/// Ask a model, record its answer, execute and record its tool calls, then repeat.
/// Recording precedes the next operation, so a failed record stops the run.
/// There is no system instruction, provider, filesystem or round budget here.
pub async fn run_turn<M, Asked, T, Called>(
    mut messages: Vec<Message>,
    mut ask_model: M,
    mut call_tool: T,
    mut append_message: impl FnMut(&Message) -> anyhow::Result<()>,
) -> anyhow::Result<()>
where
    M: FnMut(Vec<Message>) -> Asked,
    Asked: Future<Output = anyhow::Result<Answered>>,
    T: FnMut(Call) -> Called,
    Called: Future<Output = Answer>,
{
    loop {
        let answer = ask_model(messages.clone()).await?;
        let message = Message {
            role: "assistant".into(),
            text: answer.text,
            calls: answer.calls,
            thinking: answer.thinking,
            ..Message::default()
        };
        let mut ids = std::collections::BTreeSet::new();
        anyhow::ensure!(
            message
                .calls
                .iter()
                .all(|c| !c.id.is_empty() && ids.insert(&c.id)),
            "tool call IDs must be unique and nonempty"
        );
        append_message(&message)?;
        messages.push(message.clone());
        anyhow::ensure!(!answer.interrupted, "the model's answer was interrupted");
        if message.calls.is_empty() {
            return Ok(());
        }
        for call in message.calls {
            let id = call.id.clone();
            let answer = call_tool(call).await;
            anyhow::ensure!(answer.call == id, "the tool answered a different call");
            let message = Message {
                role: "user".into(),
                answers: vec![answer],
                ..Message::default()
            };
            append_message(&message)?;
            messages.push(message);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/core/mod.rs"]
mod tests;
