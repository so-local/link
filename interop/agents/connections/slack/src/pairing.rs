//! Pair on the live Socket Mode connection, before the bridge starts.
use anyhow::Context;
use std::time::Duration;

pub struct Pairing {
    socket: crate::socket::Socket,
}

/// Pairing milestones, carrying no credentials, code or message contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingProgress {
    Connected,
    OtherEvent,
    UnmatchedMessage,
    Confirming,
}

impl PairingProgress {
    pub fn text(self) -> &'static str {
        match self {
            Self::Connected => "Slack Socket Mode is connected; waiting for your direct message.",
            Self::OtherEvent => {
                "Slack delivered an event that isn't a pairing direct message; still waiting for your code."
            }
            Self::UnmatchedMessage => {
                "Slack delivered a direct message that doesn't match the pairing code. Send the code alone as a new message."
            }
            Self::Confirming => "Slack received the pairing code; sending its confirmation.",
        }
    }
}

impl Pairing {
    /// Open before printing the code, so a quick DM is already queued.
    pub async fn open(app_token: &str) -> anyhow::Result<Self> {
        Ok(Self {
            socket: crate::socket::open(app_token).await?,
        })
    }

    pub async fn owner(
        &mut self,
        token: &str,
        code: &str,
        limit: Duration,
        mut progress: impl FnMut(PairingProgress),
    ) -> anyhow::Result<(String, String, String)> {
        let waiting = async {
            while let Some(incoming) = crate::socket::event(&mut self.socket).await? {
                let payload = match incoming {
                    crate::socket::Event::Connected => {
                        progress(PairingProgress::Connected);
                        continue;
                    }
                    crate::socket::Event::Message(payload) => payload,
                };
                let event = &payload["event"];
                if event["type"] != "message" || event["channel_type"] != "im" {
                    progress(PairingProgress::OtherEvent);
                    continue;
                }
                let Some(user) = crate::sent_code(event, code) else {
                    progress(PairingProgress::UnmatchedMessage);
                    continue;
                };
                let Some(channel) = event["channel"].as_str().filter(|c| !c.is_empty()) else {
                    progress(PairingProgress::OtherEvent);
                    continue;
                };
                progress(PairingProgress::Confirming);
                let name = crate::name_of(token, &user).await;
                crate::call(
                    token,
                    "chat.postMessage",
                    &[
                        ("channel", channel),
                        ("text", "Got it: only you can talk to me now."),
                    ],
                )
                .await
                .context("Slack received the code but couldn't send the confirmation")?;
                return Ok((user, name, channel.to_string()));
            }
            anyhow::bail!(
                "Slack closed Socket Mode before pairing; run lnk agent connect slack again"
            )
        };
        tokio::time::timeout(limit, waiting).await.map_err(|_| {
            anyhow::Error::from(link_connection_contract::Expired {
                connection: "slack".into(),
                waited: limit,
            })
            .context("check the app's message.im event subscription and its Messages tab")
        })?
    }
}
