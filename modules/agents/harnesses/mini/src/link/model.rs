//! The model seam: Mini supplies a prompt; Link owns provider protocols and caching.

use crate::core::{Answered, Message};
use link_harness_contract::harness::Model;
use link_model_contract::{
    Known,
    chat::{Prompt, Tool, client},
};
use tokio::sync::mpsc;

pub(crate) struct Request<'a> {
    pub client: &'a reqwest::Client,
    pub model: &'a Model,
    pub known: Option<&'a Known>,
    /// The window a turn fills, as the agent's spec says, if it says.
    pub window: Option<u32>,
    pub instructions: &'a str,
    pub tools: &'a [Tool],
    pub pieces: &'a mpsc::Sender<String>,
    pub thread: &'a str,
}

impl Request<'_> {
    /// Ask Link's typed model interface. Standalone fixtures use its shared client.
    pub(crate) async fn ask_model(&self, messages: &[Message]) -> anyhow::Result<Answered> {
        let defaults = super::defaults();
        // The spec's window first: Link's sandbox refuses a request for
        // more than it grants, which is that.
        let window = self
            .window
            .or_else(|| self.known.and_then(|known| known.facts.window))
            .unwrap_or(defaults.window);
        let answer = self
            .known
            .and_then(|known| known.facts.answer)
            .unwrap_or(defaults.answer)
            .min(defaults.answer)
            .min(window);
        let prompt = Prompt {
            system: self.instructions,
            messages,
            tools: self.tools,
            no_calls: false,
            answer,
            window,
            price: None,
            above: &[],
            thinking: Default::default(),
            fast: false,
            chat: Some(self.thread),
            spans: &[],
            parent: None,
            cache: None,
        };
        let (_cancel, cancelled) = tokio::sync::watch::channel(false);
        match &self.model.actions_url {
            Some(url) => {
                client::canonical::answer(
                    &client::canonical::client()?,
                    client::canonical::Target {
                        url,
                        name: self.model.named().as_deref(),
                    },
                    &prompt,
                    self.pieces,
                    false,
                    cancelled,
                    None,
                )
                .await
            }
            None => {
                client::answer(
                    self.client,
                    &client::Model {
                        provider: self.model.provider.clone(),
                        name: self.model.named(),
                        base_url: self.model.base_url.clone(),
                        api_key: self.model.api_key.duplicate(),
                    },
                    &prompt,
                    self.pieces,
                    false,
                    cancelled,
                )
                .await
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/link/model.rs"]
mod tests;
