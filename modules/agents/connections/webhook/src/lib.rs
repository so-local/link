//! `lnk-webhook-connection`: the module of an agent's own webhooks (`lnk agent
//! webhook`). It speaks the pushed connection contract
//! ([`link_connection_contract::pushed`]) with one verb, `sift`: each post the
//! agent's bridge takes at a webhook's address is checked here, as the
//! webhook's sender signs (Link, or GitHub), with `link_secretive::inbound`.
//! It reads and calls nothing, and holds no way to the agent: the bridge
//! hands a post it took to the agent itself, as `webhook:<name>`.

use link_connection_contract::pushed::{self, Sifting, Verdict};
use link_connection_contract::webhook::{PREFIX, Settings};
use link_secretive::inbound::{Inbound, Receiver, Refused};
use serde::Deserialize;

/// The answer to a post signed right that carries neither JSON nor text.
pub const NOT_JSON_OR_TEXT: u16 = 415;

/// `sift` for one post, now.
pub fn sift(sifting: &Sifting) -> Verdict {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    sift_at(sifting, now)
}

/// `sift` for one post, `now` seconds since 1970: taken only if it's
/// signed with the webhook's secret, or its previous one while it rolls,
/// as its sender signs, and carries JSON or text. Its id is the one its
/// scheme signs: Link's `link-id`, or for GitHub, which signs none, the
/// body's SHA-256.
pub fn sift_at(sifting: &Sifting, now: u64) -> Verdict {
    // A webhook the bridge describes wrongly takes nothing.
    let unconfigured = Verdict::drop(Refused::Unconfigured.status());
    let Ok(hook) = Settings::deserialize(&sifting.settings) else {
        return unconfigured;
    };
    if link_machine::machine::check_name(&hook.name).is_err() {
        return unconfigured;
    }
    let Some(receiver) = Receiver::new(format!("{PREFIX}:{}", hook.name)) else {
        return unconfigured;
    };
    let inbound = Inbound {
        receiver,
        scheme: hook.from.scheme(),
        max_body: pushed::UPDATE_MAX,
        secrets: sifting.secrets(),
    };
    let verified = match inbound.verify(&sifting.headers, sifting.body.as_bytes(), now) {
        Ok(verified) => verified,
        Err(refused) => return Verdict::drop(refused.status()),
    };
    // Signed: only now is what it carries looked at, so a stranger learns
    // nothing but that it isn't signed.
    if let Err(status) = carries_json_or_text(sifting) {
        return Verdict::drop(status);
    }
    // Both schemes read an id; a post with none is never taken.
    let Some(dedupe) = verified.id.filter(|id| !id.is_empty()) else {
        return Verdict::drop(Refused::Malformed.status());
    };
    Verdict::Take {
        thread: format!("{PREFIX}:{}", hook.name),
        chat: hook.name,
        dedupe,
        // Who it's from is the webhook's, which the bridge says.
        from: None,
    }
}

/// Whether the post carries JSON (`application/json`, or a `+json` type,
/// and its body is JSON) or text (`text/*`): an error with its answer if
/// not. A body is UTF-8 already: the bridge refuses any other.
fn carries_json_or_text(sifting: &Sifting) -> Result<(), u16> {
    let essence = sifting
        .headers
        .get("content-type")
        .and_then(|value| value.split(';').next())
        .map(|essence| essence.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let json = essence == "application/json"
        || (essence.starts_with("application/") && essence.ends_with("+json"));
    if json {
        return serde_json::from_str::<serde::de::IgnoredAny>(&sifting.body)
            .map(|_| ())
            .map_err(|_| Refused::Malformed.status());
    }
    match essence.starts_with("text/") {
        true => Ok(()),
        false => Err(NOT_JSON_OR_TEXT),
    }
}

#[cfg(test)]
link_test_kit::program!(unit);

/// `lnk-webhook-connection`'s command line, generated from its
/// declarations: the connection protocol's `sift`, which its contract
/// declares (`actions.toml`), whose shapes are the contract's
/// (`link_connection_contract::declared`).
pub mod cli {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));

    /// Whether this run is the connection protocol's `sift`, its one
    /// command: it keeps nothing of its own to remove, so core never asks
    /// it to uninstall.
    pub fn sifts() -> anyhow::Result<bool> {
        let parsed: link_spec::Parsed<Cli> = PROGRAM.parse();
        match parsed.command.command {
            Command::WebhookConnectionSift(_) => Ok(true),
        }
    }

    /// `lnk-webhook-connection`'s whole command line, as clap sees it.
    pub fn command() -> clap::Command {
        <Cli as clap::CommandFactory>::command()
    }
}
