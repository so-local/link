//! `lnk model bridge`: the bridge the model routes go to
//! ([`link_model_contract::route`]), one for this machine's user, which
//! the first proxy whose call needs it starts. A hosted model's call gets
//! its key added and goes to its provider over TLS ([`hosted`]); a local
//! runtime's goes to its port, for inference only ([`local`]). What each
//! answer counts is taken out of it ([`usage`]) and handed to the measure
//! module, as the calling agent's.

pub mod actions;
pub mod context;
pub mod hosted;
pub mod local;
pub mod show;
pub mod usage;

use std::path::Path;
use std::time::Duration;

use anyhow::bail;
use link_model_contract::route::{Hosted, HostedKey, Local};
use link_sandbox_binding::Identity;
use link_sandbox_binding::Limits;
use link_sandbox_route::Call;
use link_sandbox_route::To;
use link_sandbox_route::Verdict;
use link_sandbox_route::Verdicts;
use link_sandbox_route::bridge::Started;
use link_sandbox_route::pool::Kept;
use link_sandbox_route::pool::Pool;
use link_secretive::Duplicate;

/// What the bridge takes at once: every running agent's model calls, a
/// turn of each streaming its answer.
pub const LIMITS: Limits = Limits {
    name: "the model bridge",
    calls: 256,
    per_agent: 32,
    per_owner: 128,
    action_max: 1 << 20,
    action_wait: Duration::from_secs(10),
    answer_wait: Duration::from_secs(10),
};

/// How long it runs with no call: then it's gone, and the next call
/// starts it again.
pub const IDLE: Duration = Duration::from_secs(10 * 60);

/// The connections it keeps to providers, by host and upstream.
static KEPT: Pool<Kept> = Pool::new();

/// Serves the bridge as `started` says, until idle.
pub async fn serve(started: &Started) -> anyhow::Result<()> {
    let settings = &started.settings;
    link_sandbox_route::bridge::serve(started, &LIMITS, IDLE, |who, call, stream| {
        handle(settings, who, call, stream)
    })
    .await
}

/// The key `of` names: the model's or fallback's of the agent acting as
/// `service`, as the store grants it (an error when it grants none), or
/// the one handed.
fn granted_key(service: &str, of: &HostedKey) -> anyhow::Result<link_secretive::Secret> {
    use link_agent_identity::own::OwnSecret;
    let want = match of {
        HostedKey::Model => OwnSecret::Model.to_use()?,
        HostedKey::Fallback => OwnSecret::Fallback.to_use()?,
        HostedKey::Handed(key) => return Ok(key.duplicate()),
    };
    link_secret_contract::for_service(service, link_secret_contract::Which::Own)?
        .into_iter()
        .find(|given| given.r#as == want)
        .map(|given| given.value)
        .ok_or_else(|| anyhow::anyhow!("no model key is granted to this proxy's caller"))
}

/// One call, from `who`: its route's, its tokens added to `who`'s count
/// in Link's `settings`.
async fn handle(
    settings: &Path,
    who: Identity,
    call: Call,
    mut stream: link_sandbox_binding::stream::Stream,
) -> anyhow::Result<()> {
    let counter = usage::Counter::new();
    let served = match call.route.as_str() {
        name if name == "model-action" || name.starts_with("model-action-") => {
            let grant = link_sandbox_route::given(&call.given)?;
            let To::Request { head } = &call.to else {
                bail!("model actions take requests");
            };
            return actions::serve(
                stream,
                head.as_bytes(),
                Vec::new(),
                &grant,
                settings,
                &who.agent,
                &who.service,
                &call.upstream,
            )
            .await;
        }
        "model" | "model-at" | "model-at-fallback" => {
            let hosted: Hosted = link_sandbox_route::given(&call.given)?;
            let To::Request { head } = &call.to else {
                bail!("a hosted model's call is a request");
            };
            // The caller's key, read at this call from what the store
            // grants it, off the runtime's threads: a file, the Keychain
            // or the keeper.
            let key = match &hosted.key {
                HostedKey::Handed(key) => Ok(key.duplicate()),
                named => {
                    let (service, of) = (who.service.clone(), named.duplicate());
                    tokio::task::spawn_blocking(move || granted_key(&service, &of)).await?
                }
            }
            .and_then(|key| hosted.with_key(key));
            // Not granted its caller (any more), or not the key its
            // placeholder was made for: refused, saying why.
            let key = match key {
                Ok(key) => key,
                Err(err) => {
                    return local::refuse(&mut stream, String::new(), &format!("{err:#}")).await;
                }
            };
            let upstream = link_sandbox_route::dial::upstream(&call.upstream)?;
            let (head, rest) = (head.as_bytes(), Vec::new());
            hosted::relay_to(
                stream,
                head,
                rest,
                &key,
                call.listed,
                &KEPT,
                &call.upstream,
                &counter,
                |host| async move { link_sandbox_route::tls::connect(&host, upstream).await },
            )
            .await
        }
        "local-model" => {
            let local: Local = link_sandbox_route::given(&call.given)?;
            match &call.to {
                To::Request { head } => {
                    let port = local::unprefixed(head.as_bytes());
                    match port {
                        Ok((port, head)) if local.ports.contains(&port) => {
                            local::relay_request(stream, &head, Vec::new(), port, &counter).await
                        }
                        _ => {
                            let why = format!(
                                "{} isn't a model runtime of this sandbox's",
                                head.split(' ').nth(1).unwrap_or_default()
                            );
                            local::refuse(&mut stream, String::new(), &why).await
                        }
                    }
                }
                To::Port { port } if local.ports.contains(port) => {
                    local::relay(stream, *port, &counter).await
                }
                To::Port { port } => {
                    let verdict = Verdict::refused(format!("127.0.0.1:{port}"));
                    stream.say(&verdict).await?;
                    bail!("refused: port {port} isn't a model runtime of this sandbox's")
                }
            }
        }
        other => bail!("the model bridge serves no route {other:?}"),
    };
    let counted = counter.now();
    if counted != link_measure_contract::client::Tokens::default() {
        link_measure_contract::client::tokens::add(settings, &who.agent, &counted)?;
    }
    // What it asked, kept once it was answered, with what the answer
    // measured if it said.
    if let Some(asked) = counter.take_asked()
        && counted.requests > 0
    {
        let tokens = (counted.unmetered == 0).then_some(link_model_contract::chat::Tokens {
            input: counted.input,
            output: counted.output,
            cached: counted.cached,
        });
        asked.keep(settings, &who.agent, tokens)?;
    }
    served
}
