//! A pushed connection's signature check. [`main`] reads the shared sift
//! line protocol; [`sift`] verifies the post and returns an accept-or-drop
//! verdict. Link's bridge delivers accepted posts, so no `take` is needed.
//!
//! Tests run this fixture as `lnk-webhook-connection`, separate from the released module.
//! It accepts Link signatures, or GitHub signatures when settings name GitHub.

use link_connection_contract::{
    pushed::{self, Sifting, Verdict},
    webhook,
};
use link_secretive::inbound::{Inbound, Receiver, Scheme};

#[derive(clap::Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(clap::Subcommand)]
enum Command {
    Sift,
}

fn main() -> anyhow::Result<()> {
    let Cli {
        command: Command::Sift,
    } = link_spec::entry::plain();
    pushed::sift_lines(sift, std::io::stdin().lock(), std::io::stdout())
}

/// The webhook's name, and how its sender signs.
fn of(settings: &serde_json::Value) -> Option<(String, Scheme)> {
    let settings: webhook::Settings = serde_json::from_value(settings.clone()).ok()?;
    Some((settings.name, settings.from.scheme()))
}

fn sift(sifting: &Sifting) -> Verdict {
    let Some((name, scheme)) = of(&sifting.settings) else {
        return Verdict::drop(400);
    };
    let Some(receiver) = Receiver::new(format!("webhook:{name}")) else {
        return Verdict::drop(400);
    };
    let inbound = Inbound {
        receiver,
        scheme,
        max_body: pushed::UPDATE_MAX,
        secrets: sifting.secrets(),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    match inbound.verify(&sifting.headers, sifting.body.as_bytes(), now) {
        Ok(verified) => Verdict::Take {
            thread: format!("webhook:{name}"),
            chat: name,
            dedupe: verified.id.unwrap_or_default(),
            // Who it's from is the webhook's, which the bridge says.
            from: None,
        },
        Err(refused) => Verdict::drop(refused.status()),
    }
}
