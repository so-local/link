//! A route's call, handed to its bridge ([`link_sandbox_route`]): the
//! proxy reaches the bridge (starting it if nothing listens), proves who
//! calls ([`Calling::caller`]), sends the call ([`link_sandbox_route::Call`])
//! and what the client sends after it as it comes, reads the bridge's
//! verdict for its log, and carries the bridge's answer back as it comes.
//! A client that leaves (hangs up, where that can be told from shutting
//! its writing) ends the call, and the bridge's connection with it.

use anyhow::{Context, bail};
use link_sandbox_route::Call;
use link_sandbox_route::Outcome;
use link_sandbox_route::Route;
use link_sandbox_route::To;
use link_sandbox_route::Verdict;
use link_sandbox_route::io::Activity;
use link_sandbox_route::io::until_hung_up;
use link_sandbox_route::io::until_idle;
use tokio::io::AsyncWriteExt;

use crate::filter::Filter;
use crate::http::reply;
use crate::routes::Client;

/// Who a proxy's route calls are from, and what each says of its
/// sandbox: the same for every call it carries, and as long-lived.
#[derive(Debug)]
pub struct Calling {
    /// Who calls, proved to each bridge.
    pub caller: link_sandbox_binding::Binding,
    /// The sandbox's name on its wire.
    pub sandbox: String,
    /// Where the bridges' own connections for its calls leave from.
    pub upstream: String,
    /// The socket into the sandbox's own network (Linux).
    pub inbound: Option<link_sandbox_route::Inbound>,
}

/// The call `route`'s bridge is handed for `to`: `listed` unless the
/// sandbox reaches every public host, so a sandbox with no internet is
/// never taken for one that reaches any.
fn call_of(route: &Route, calling: &Calling, filter: &Filter, to: To) -> Call {
    Call {
        route: route.name.clone(),
        sandbox: calling.sandbox.clone(),
        upstream: calling.upstream.clone(),
        listed: !filter.every_host(),
        given: route.given.duplicate(),
        to,
        inbound: calling.inbound.clone(),
    }
}

/// Serves one call on `route` from `client`: `to` what, `early` what came
/// with it already. Logged in `call` by the bridge's verdict.
pub async fn serve<S>(
    mut client: S,
    route: &'static Route,
    to: To,
    early: Vec<u8>,
    filter: Filter,
    call: &mut crate::wire::Call,
) -> anyhow::Result<()>
where
    S: Client,
{
    let Some(calling) = filter.calling else {
        reply(&mut client, "403 Forbidden").await;
        bail!("refused: this proxy calls no routes");
    };
    let action = call_of(route, calling, &filter, to);
    let mut bridge = match open(route, &calling.caller, &action).await {
        Ok(bridge) => bridge,
        Err(err) => {
            unreachable(&mut client, route, &err).await;
            return Err(err);
        }
    };
    let hangup = client.hangup_socket();
    let (mut from_bridge, mut to_bridge) = bridge.split();
    let (mut from_client, mut to_client) = tokio::io::split(client);
    let activity = Activity::new();
    let upload = async {
        let mut to_bridge = activity.watch(&mut to_bridge);
        let sent = async {
            to_bridge.write_all(&early).await?;
            tokio::io::copy(&mut activity.watch(&mut from_client), &mut to_bridge).await
        };
        // A bridge that refuses answers and closes before reading the
        // rest: the write fails, and its answer, still coming, says why.
        // Shut its writing otherwise: a half-close, after which it reads
        // its answer.
        if sent.await.is_ok() {
            let _ = to_bridge.shutdown().await;
        }
        match hangup {
            Some(socket) => until_hung_up(socket).await,
            None => std::future::pending().await,
        }
        anyhow::Ok(Outcome::Allowed)
    };
    let answer = async {
        let verdict: Verdict = match link_sandbox_binding::stream::verdict(&mut from_bridge).await {
            Ok(verdict) => verdict,
            Err(err) => {
                let mut to_client = activity.watch(&mut to_client);
                refused(&mut to_client, &err).await;
                return Err(err.context(format!("the {} route's bridge", route.name)));
            }
        };
        if !verdict.to.is_empty() {
            call.to = verdict.to;
        }
        if verdict.outcome == Outcome::Allowed {
            call.allowed();
        }
        tokio::io::copy(
            &mut activity.watch(&mut from_bridge),
            &mut activity.watch(&mut to_client),
        )
        .await?;
        let _ = to_client.shutdown().await;
        anyhow::Ok(verdict.outcome)
    };
    let carried = async {
        tokio::select! {
            // The client left: the call ends, the bridge's with it.
            left = upload => left.and_then(|_| bail!("the client left")),
            answered = answer => answered,
        }
    };
    match until_idle(&activity, carried).await? {
        Outcome::Allowed => Ok(()),
        Outcome::Refused => bail!("refused: by the {} route's bridge", route.name),
        Outcome::Failed => bail!("the {} route's bridge couldn't reach it", route.name),
    }
}

/// A connection to `route`'s bridge, its call `action` sent, proved as
/// `caller`.
async fn open(
    route: &Route,
    caller: &link_sandbox_binding::Binding,
    action: &Call,
) -> anyhow::Result<tokio::net::UnixStream> {
    let mut bridge = link_sandbox_route::bridge::reach(&route.bridge).await?;
    link_sandbox_binding::stream::open(&mut bridge, caller, action)
        .await
        .with_context(|| format!("calling the {} route's bridge", route.name))?;
    Ok(bridge)
}

/// Tells the client its route's bridge couldn't be reached: 502.
async fn unreachable<S: Client>(client: &mut S, route: &Route, err: &anyhow::Error) {
    let text = format!(
        "Link's sandbox: the {} route's bridge couldn't be reached: {}\n",
        route.name,
        link_format::clean(&format!("{err:#}"))
    );
    answer(client, "502 Bad Gateway", &text).await;
}

/// Tells the client its call was refused before an answer (the bridge's
/// failure, whose reason it can read): 403 for a call refused, else 502.
async fn refused<S: tokio::io::AsyncWrite + Unpin>(client: &mut S, err: &anyhow::Error) {
    use link_sandbox_binding::Failure;
    use link_sandbox_binding::Why;
    let status = match err.downcast_ref::<Failure>() {
        Some(failure) if failure.why == Why::Refused => "403 Forbidden",
        Some(failure) if failure.why == Why::Busy => "503 Service Unavailable",
        _ => "502 Bad Gateway",
    };
    let text = format!(
        "Link's sandbox: {}\n",
        link_format::clean(&format!("{err:#}"))
    );
    answer(client, status, &text).await;
}

pub(crate) async fn answer<S: tokio::io::AsyncWrite + Unpin>(
    client: &mut S,
    status: &str,
    text: &str,
) {
    let _ = client
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{text}",
                text.len()
            )
            .as_bytes(),
        )
        .await;
}
