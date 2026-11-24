//! Model namespace client; the harness only projects its history and keeps its answer.
use crate::event::{self, Event, Kind, Media, Part};
#[cfg(test)]
use crate::event::{Call, Tokens};
#[cfg(test)]
use crate::tools::Tool;
pub use client::{Overloaded, TooLong, Unreached, asked_to_end};
use link_harness_contract::harness::Model;
use link_model_contract::chat::client;
pub use link_model_contract::chat::{
    Answer, Message, PARTS_SENT_MAX, Prompt, SUMMARY_HEAD, Think, UNANSWERED,
};
#[cfg(test)]
use serde_json::{Value, json};
use tokio::sync::mpsc;
pub mod digests;
mod history;
pub use digests::Digests;
pub use history::{Projection, messages, projected, projected_kept, speaker};

pub trait RequestObserver: Send + Sync {
    fn requesting(&self, model: &Model, prompt: &Prompt<'_>) -> anyhow::Result<u64>;
}

impl<F> RequestObserver for F
where
    F: Fn(&Model, &Prompt<'_>) -> anyhow::Result<u64> + Send + Sync,
{
    fn requesting(&self, model: &Model, prompt: &Prompt<'_>) -> anyhow::Result<u64> {
        self(model, prompt)
    }
}
fn target(model: &Model) -> client::Model {
    client::Model {
        provider: model.provider.clone(),
        name: model.named(),
        base_url: model.base_url.clone(),
        api_key: model.api_key.duplicate(),
    }
}
pub async fn answer(
    client: &reqwest::Client,
    model: &Model,
    prompt: &Prompt<'_>,
    pieces: &mpsc::Sender<String>,
    separate: bool,
    cancelled: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<Event> {
    answer_within(client, model, prompt, pieces, separate, cancelled, None).await
}
pub async fn answer_within(
    client: &reqwest::Client,
    model: &Model,
    prompt: &Prompt<'_>,
    pieces: &mpsc::Sender<String>,
    separate: bool,
    cancelled: tokio::sync::watch::Receiver<bool>,
    first: Option<std::time::Duration>,
) -> anyhow::Result<Event> {
    answer_observed(
        client, model, prompt, pieces, separate, cancelled, first, None,
    )
    .await
}

#[expect(
    clippy::too_many_arguments,
    reason = "model call inputs and its optional durable observation boundary"
)]
pub async fn answer_observed(
    client: &reqwest::Client,
    model: &Model,
    prompt: &Prompt<'_>,
    pieces: &mpsc::Sender<String>,
    separate: bool,
    cancelled: tokio::sync::watch::Receiver<bool>,
    first: Option<std::time::Duration>,
    observer: Option<&dyn RequestObserver>,
) -> anyhow::Result<Event> {
    let request = observer
        .map(|observer| observer.requesting(model, prompt))
        .transpose()?;
    let reply = match &model.actions_url {
        Some(url) => {
            client::canonical::answer(
                &client::canonical::client()?,
                client::canonical::Target {
                    url,
                    name: model.named().as_deref(),
                },
                prompt,
                pieces,
                separate,
                cancelled,
                first,
            )
            .await?
        }
        None => {
            client::answer_within(
                client,
                &target(model),
                prompt,
                pieces,
                separate,
                cancelled,
                first,
            )
            .await?
        }
    };
    Ok(Event {
        request,
        role: event::ASSISTANT.into(),
        text: reply.text,
        calls: reply.calls,
        thinking: reply.thinking,
        at: event::now(),
        model: reply.model,
        provider: Some(model.provider.clone()),
        response_id: reply.response_id,
        tokens: Some(reply.tokens),
        latency_ms: Some(reply.latency_ms),
        cost: reply.cost,
        interrupted: reply.interrupted,
        ..Event::default()
    })
}
pub async fn listed(client: &reqwest::Client, model: &Model) -> anyhow::Result<Vec<String>> {
    match &model.actions_url {
        Some(url) => client::canonical::get(&client::canonical::client()?, url, "list").await,
        None => client::listed(client, &target(model)).await,
    }
}
pub async fn heard(
    client: &reqwest::Client,
    model: &Model,
    transcriber: &str,
    voice: &Media,
) -> anyhow::Result<String> {
    match &model.actions_url {
        Some(url) => Ok(client::canonical::post::<link_model_contract::api::Heard>(
            &client::canonical::client()?,
            url,
            "transcribe",
            &link_model_contract::api::Transcribe {
                name: transcriber.into(),
                audio: voice.clone(),
            },
        )
        .await?
        .text),
        None => client::heard::heard(client, &target(model), transcriber, voice).await,
    }
}
/// Gives each of `event`'s calls an id any provider takes, unique in this
/// response: letters, digits, `_` and `-`, and `call-<seq>-<n>` for
/// one the provider gave none.
pub fn settle_ids(event: &mut Event) -> anyhow::Result<()> {
    link_model_contract::chat::calls::settle_ids(&mut event.calls, event.seq)
}

#[cfg(test)]
use crate::limits::Thinks;
#[cfg(test)]
use client::{Reply, anthropic_body, openai_body, too_long, with_causes};
#[cfg(test)]
use link_agent_spec::Thinking;
#[cfg(test)]
fn anthropic_url(model: &Model) -> String {
    client::anthropic_url(&target(model))
}
#[cfg(test)]
fn openai_url(model: &Model) -> anyhow::Result<String> {
    client::openai_url(&target(model))
}
#[cfg(test)]
mod tests;
