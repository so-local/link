//! `lnk secret bridge`: the bridge the secret route goes to
//! ([`link_secret_contract::route`]), one for this machine's user, which
//! the first proxy whose call needs it starts: a service's secret added
//! outside the sandbox.
//!
//! A program calling a service whose secret Link keeps out of the sandbox
//! is set up with its proxy as the service's root
//! ([`link_secret_contract::route::url`]) and a placeholder in place of the
//! secret ([`Sealed::placeholder`], random for each run and holder). Each
//! call arrives as `/_link/secret/<name>/<path>`, which its proxy hands
//! here with the secrets its bind gave it. The bridge checks the path
//! against the secret's rule ([`SecretRule`], data the module owning the
//! service gives, or the secret's own sealing), puts the real secret in
//! the path or a header, and speaks TLS to the rule's one host itself, so
//! the secret is never in the sandbox.
//!
//! The rule's paths pass as they are, long polls and uploads too, but
//! for those it refuses: calls that would hand what the secret guards to
//! something outside Link.

use std::time::Duration;

use anyhow::bail;
use link_sandbox_binding::Identity;
use link_sandbox_binding::Limits;
use link_sandbox_route::Call;
use link_sandbox_route::To;
use link_sandbox_route::Verdict;
use link_sandbox_route::Verdicts;
use link_sandbox_route::http::RequestHead;
use link_sandbox_route::pool::Kept;
use link_sandbox_route::pool::Pool;
use link_secret_contract::route::{Given, PREFIX, Sealed, SecretRest, SecretRule};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

/// What the bridge takes at once: every running workload's calls,
/// long polls among them.
pub const LIMITS: Limits = Limits {
    name: "the secret bridge",
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

/// The connections it keeps to services, by host and upstream.
static KEPT: Pool<Kept> = Pool::new();

/// Serves the bridge as `started` says, until idle.
pub async fn serve(started: &link_sandbox_route::bridge::Started) -> anyhow::Result<()> {
    link_sandbox_route::bridge::serve(started, &LIMITS, IDLE, |who, call, stream| {
        handle(Granted::Yours, who, call, stream)
    })
    .await
}

/// Where the bridge reads what the store grants a call's caller: your
/// secrets, wherever they're kept (your files, or the keeper's for you),
/// or, for the keeper's own bridge, the store it keeps for one user.
pub(crate) enum Granted {
    Yours,
    Kept(link_secret_contract::Store),
}

impl Granted {
    /// What it grants the service whose ID is `service`, of `which`
    /// kind, each with its value.
    fn to(
        &self,
        service: &str,
        which: link_secret_contract::Which,
    ) -> anyhow::Result<Vec<link_secret_contract::ForService>> {
        match self {
            Granted::Yours => link_secret_contract::for_service(service, which),
            Granted::Kept(store) => {
                let request = link_secret_contract::keeper::Request::ForService {
                    id: service.to_string(),
                    which,
                };
                store.answer(request, "")?.for_service()
            }
        }
    }
}

/// The own access token of the workload whose ID is `workload`, for
/// `host`, granted `grants`, as its machine signs for it
/// ([`super::tokens::installation_signed`]), or one traded for `assertion`
/// where it has one.
fn caller_token(
    workload: &str,
    host: &str,
    grants: &str,
    assertion: Option<&link_secret_contract::route::Assertion>,
) -> anyhow::Result<link_secretive::Secret> {
    match assertion {
        Some(assertion) => super::tokens::exchange(assertion, host, grants),
        None => super::tokens::installation_signed(workload, host, grants),
    }
}

/// One call on the secret route from `who`: the secret its target names
/// added to it, as its proxy was handed it, or by name, read at this call
/// from what `granted` grants `who`, so a secret or grant removed stops
/// at the next call.
pub(crate) async fn handle(
    granted: Granted,
    who: Identity,
    call: Call,
    mut stream: link_sandbox_binding::stream::Stream,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        call.route == "secret",
        "the secret bridge serves no route {:?}",
        call.route
    );
    let given: Given = link_sandbox_route::given(&call.given)?;
    for secret in &given.secrets {
        check(secret)?;
    }
    let To::Request { head } = &call.to else {
        bail!("a secret's call is a request");
    };
    let target = RequestHead::parse(head.as_bytes())
        .map(|request| request.target.to_string())
        .unwrap_or_default();
    // Read off the runtime's threads: the store is a file, the Keychain
    // or the keeper.
    let found = tokio::task::spawn_blocking(move || {
        given.find(
            &target,
            |which| granted.to(&who.service, which),
            |host, grants, assertion| match &granted {
                Granted::Yours => caller_token(&who.agent, host, grants, assertion),
                // The keeper is a user of its own, signed in nowhere.
                Granted::Kept(_) => anyhow::bail!("a token is made by your own secret bridge"),
            },
        )
    })
    .await?;
    // Not granted its caller (any more), or not the value its
    // placeholder was made for: refused, saying why.
    let found = match found {
        Ok(found) => found,
        Err(err) => return refuse(&mut stream, String::new(), &format!("{err:#}")).await,
    };
    let Some(secret) = found else {
        let why = "no such secret on this sandbox's proxy";
        return refuse(&mut stream, String::new(), why).await;
    };
    check(&secret)?;
    let secret = &secret;
    let upstream = link_sandbox_route::dial::upstream(&call.upstream)?;
    // A socket its rule takes the secret in the first message of.
    if let Some(socket) = &secret.rule.socket
        && crate::socket::is_upgrade(head.as_bytes())
    {
        let host = socket.host.clone();
        return crate::socket::relay(stream, head.as_bytes(), secret, || async move {
            link_sandbox_route::tls::connect(&host, upstream).await
        })
        .await;
    }
    let host = secret.rule.host.clone();
    relay_to(
        stream,
        head.as_bytes(),
        Vec::new(),
        secret,
        &KEPT,
        &call.upstream,
        || async move { link_sandbox_route::tls::connect(&host, upstream).await },
    )
    .await
}

/// The secret named in `target` (a path on the proxy), and the path
/// after its name; None when the target names none.
pub fn named(target: &str) -> Option<(&str, &str)> {
    target.strip_prefix(PREFIX)?.split_once('/')
}

/// Whether `text` can be put in a path as it is: letters, digits, `:`,
/// `_` and `-`.
fn path_safe(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":_-".contains(&b))
}

/// Whether `text` can be put in a header as it is: visible ASCII.
fn header_safe(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| (0x21..0x7f).contains(&b))
}

/// Whether `rest` is what `kind` lets follow a prefix.
fn rest_allowed(rest: &str, kind: SecretRest) -> bool {
    match kind {
        SecretRest::Name => !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric()),
        SecretRest::Path => {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-./".contains(&b))
                && !rest
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
        }
    }
}

/// The request line's target to send the service for `target` (the path
/// on the proxy), the real secret in place of the placeholder when it
/// goes in the path; or why not. Paths are taken as sent: an encoded or
/// relative one is refused.
pub fn rewrite(target: &str, secret: &Sealed) -> Result<String, String> {
    let rule = &secret.rule;
    let Some((name, rest)) = named(target) else {
        return Err(format!("{target} isn't on the wire"));
    };
    if name != secret.name {
        return Err(format!("no secret {name:?} on this sandbox's proxy"));
    }
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    let in_path = rule.header.is_none();
    if in_path && !path_safe(secret.value.expose()) {
        return Err("a secret of letters, digits, :, _ and - only".into());
    }
    let expected = secret.placeholder.expose();
    let mut shaped = false;
    for allowed in &rule.paths {
        // The part of the path standing for the secret, if the path has
        // this one's shape, and what follows the prefix.
        let (placeholder, after) = match allowed.prefix.split_once("{secret}") {
            Some((before, after_secret)) if in_path => {
                let Some(from) = path.strip_prefix(before) else {
                    continue;
                };
                // The rule's `{secret}/`: the secret is up to the next `/`.
                let Some((placeholder, _)) = from.split_once('/') else {
                    continue;
                };
                let Some(after) = from[placeholder.len()..].strip_prefix(after_secret) else {
                    continue;
                };
                (Some(placeholder), after)
            }
            Some(_) => continue,
            None => match path.strip_prefix(allowed.prefix.as_str()) {
                Some(after) if !in_path => (None, after),
                _ => continue,
            },
        };
        shaped = true;
        // Only the program given the placeholder gets the secret added:
        // on macOS anyone on this machine can reach the proxy's port. A
        // path with another's shape may still be another of the rule's.
        if let Some(placeholder) = placeholder
            && !(path_safe(placeholder)
                && link_secretive::equal(placeholder.as_bytes(), expected.as_bytes()))
        {
            continue;
        }
        if !rest_allowed(after, allowed.rest) {
            return Err(format!("{after:?} isn't a path this service's rule allows"));
        }
        let pattern = format!("{}{after}", allowed.prefix).to_ascii_lowercase();
        if rule
            .refuse
            .iter()
            .any(|refused| refused.to_ascii_lowercase() == pattern)
        {
            return Err(format!(
                "{after} would hand what its secret guards to something outside Link"
            ));
        }
        let sent = match in_path {
            true => allowed.prefix.replace("{secret}", secret.value.expose()),
            false => allowed.prefix.clone(),
        };
        return Ok(match query {
            Some(query) => format!("/{sent}{after}?{query}"),
            None => format!("/{sent}{after}"),
        });
    }
    Err(match shaped {
        true => "not the placeholder this sandbox was given".into(),
        false => format!("{path:?} isn't a call this service's rule allows"),
    })
}

/// The head to send the service for the program's `head`, or why not:
/// its target rewritten ([`rewrite`]), the rule's `Host`, the secret in
/// its header if it goes in one, the rest of its headers as they are but
/// for those Link sets. A request whose body's length isn't said once
/// ([`link_sandbox_route::pool::request_length`]) is the last on its connection.
pub fn prepare(head: &[u8], secret: &Sealed) -> Result<Vec<u8>, String> {
    let rule = &secret.rule;
    let request = RequestHead::parse(head)?;
    let (method, target) = (request.method, request.target);
    let allowed = match rule.methods.is_empty() {
        true => ["GET", "POST"].contains(&method),
        false => rule.methods.iter().any(|m| m == method),
    };
    if !allowed {
        return Err(format!(
            "{method} isn't a method this service's rule allows"
        ));
    }
    let target = rewrite(target, secret)?;
    let mut out = format!("{method} {target} HTTP/1.1\r\nHost: {}\r\n", rule.host);
    let secret_header = rule.header.as_ref().map(|h| h.name.to_ascii_lowercase());
    let mut shown = false;
    for line in &request.headers {
        let name = &line.name;
        if Some(name) == secret_header.as_ref() {
            let header = rule.header.as_ref().expect("a header");
            let expected = header.filled(secret.placeholder.expose());
            if shown || !link_secretive::equal(line.value.as_bytes(), expected.as_bytes()) {
                return Err("not the placeholder this sandbox was given".into());
            }
            shown = true;
            continue;
        }
        match name.as_str() {
            "upgrade" => return Err("no switching protocols".into()),
            // `expect`: the proxy answers it itself.
            "host" | "authorization" | "connection" | "keep-alive" | "te" | "trailer"
            | "expect" => {}
            other if other.starts_with("proxy-") => {}
            _ => {
                out.push_str(line.line);
                out.push_str("\r\n");
            }
        }
    }
    if let Some(header) = &rule.header {
        if !shown {
            return Err("not the placeholder this sandbox was given".into());
        }
        if !header_safe(secret.value.expose()) {
            return Err("a secret that can't go in a header".into());
        }
        let value = header.filled(secret.value.expose());
        out.push_str(&format!("{}: {value}\r\n", header.name));
    }
    if link_sandbox_route::pool::request_length(out.as_bytes()).is_none() {
        out.push_str("Connection: close\r\n");
    }
    out.push_str("\r\n");
    Ok(out.into_bytes())
}

/// One call on the route: its request's head (already read, with `rest`
/// of what came after it) prepared and sent to the rule's host over TLS
/// with the real secret, on a connection kept in `pool` from an earlier
/// call (`via` the upstream it left through) if there's one, else made
/// with `connect`, then what follows (its body) and the answer copied
/// through as they come. Its verdict, allowed, is said once the service
/// answers the connection, so a long poll is in the log while it's open.
pub async fn relay_to<S, O, F, Fut>(
    mut client: S,
    head: &[u8],
    rest: Vec<u8>,
    secret: &Sealed,
    pool: &Pool<O>,
    via: &str,
    connect: F,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Verdicts + Unpin,
    O: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<O>>,
{
    let host = secret.rule.host.as_str();
    let to = format!("{host}:443");
    let prepared = match prepare(head, secret) {
        Ok(prepared) => prepared,
        Err(why) => return refuse(&mut client, to, &why).await,
    };
    let kept = Pool::<O>::key(host, via);
    let pooled = pool.take(&kept);
    let mut origin = match pooled {
        Some(origin) => origin,
        None => match connect().await {
            Ok(origin) => origin,
            Err(err) => {
                client.say(&Verdict::failed(to)).await?;
                let _ = client
                    .write_all(
                        b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                return Err(err);
            }
        },
    };
    client.say(&Verdict::allowed(to)).await?;
    origin.write_all(&prepared).await?;
    let Some(length) = link_sandbox_route::pool::request_length(&prepared) else {
        // A body without one length: to the connection's end, which
        // closes after it.
        origin.write_all(&rest).await?;
        origin.flush().await?;
        link_sandbox_route::io::joined_until_idle(client, origin, link_sandbox_route::io::IDLE)
            .await?;
        return Ok(());
    };
    // Ended once nothing crosses either way for a while, as a tunnel is.
    let activity = link_sandbox_route::io::Activity::new();
    let reused = {
        let mut client = activity.watch(&mut client);
        let mut origin = activity.watch(&mut origin);
        let call = async {
            link_sandbox_route::pool::send_body(&mut client, &mut origin, &rest, length).await?;
            link_sandbox_route::pool::answer(&mut origin, &mut client, host).await
        };
        link_sandbox_route::io::until_idle(&activity, call).await?
    };
    if reused {
        pool.put(kept, origin);
    }
    client.shutdown().await?;
    Ok(())
}

/// Refuses the call, `why`: its verdict, to `to`, then a 403 saying why.
async fn refuse<S>(client: &mut S, to: String, why: &str) -> anyhow::Result<()>
where
    S: AsyncWrite + Verdicts + Unpin,
{
    client.say(&Verdict::refused(to)).await?;
    let body = format!("Link's sandbox: {why}\n");
    let _ = client
        .write_all(
            format!(
                "HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await;
    bail!("refused: {why}")
}

/// An error unless the bridge can add `secret`: a name, a value, a
/// placeholder Link made, a rule it can hold to.
pub fn check(secret: &Sealed) -> anyhow::Result<()> {
    link_secret_contract::route::check_secret_name(&secret.name)?;
    anyhow::ensure!(
        !secret.value.expose().trim().is_empty(),
        "secret {} is empty",
        secret.name
    );
    secret.check_placeholder()?;
    check_rule(&secret.rule)
}

/// [`SecretRule::check`], and a secret in a path followed by its `/`.
fn check_rule(rule: &SecretRule) -> anyhow::Result<()> {
    rule.check()?;
    if rule.header.is_none() {
        for path in &rule.paths {
            anyhow::ensure!(
                path.prefix.contains("{secret}/"),
                "{:?}: {{secret}} ends a part of the path",
                path.prefix
            );
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use link_secret_contract::route::{SecretHeader, SecretPath};
    use tokio::io::AsyncReadExt;

    const TOKEN: &str = "123456:AAH-real_secret";

    /// What this test run's secrets' placeholders are made with.
    pub(crate) static RUN: std::sync::LazyLock<link_sandbox_route::Placeholders> =
        std::sync::LazyLock::new(|| link_sandbox_route::Placeholders::new().unwrap());

    /// A rule the way a bot service's module gives it: methods, files,
    /// and the calls that would send the bot elsewhere refused.
    pub(crate) fn bot_rule() -> SecretRule {
        serde_json::from_value(serde_json::json!({
            "host": "api.bots.example",
            "paths": [
                {"prefix": "bot{secret}/", "rest": "name"},
                {"prefix": "file/bot{secret}/", "rest": "path"},
            ],
            "refuse": ["bot{secret}/setwebhook", "bot{secret}/logout", "bot{secret}/close"],
            "public": ":",
        }))
        .unwrap()
    }

    pub(crate) fn bot(token: &str) -> Sealed {
        link_secret_contract::route::Sealed::new(&RUN, "bot", token.to_string().into(), bot_rule())
    }

    /// The placeholder [`bot`]'s program is given for `token`.
    pub(crate) fn bot_placeholder(token: &str) -> String {
        bot(token).placeholder.expose().clone()
    }

    /// `text` with the placeholder the program is given for [`TOKEN`] in
    /// place of `bot1:x` and the old fixed ones.
    fn wired(text: &str) -> String {
        let placeholder = bot_placeholder(TOKEN);
        text.replace("bot123456:link-wire", &format!("bot{placeholder}"))
            .replace("bot1:link-wire", &format!("bot{placeholder}"))
            .replace("bot1:x", &format!("bot{placeholder}"))
    }

    #[test]
    fn the_rule_is_checked() {
        check(&bot(TOKEN)).unwrap();
        let broken = |f: fn(&mut SecretRule)| {
            let mut secret = bot(TOKEN);
            f(&mut secret.rule);
            check(&secret).is_err()
        };
        assert!(broken(|r| r.host = "*.example.com".into()));
        assert!(broken(|r| r.host = "10.0.0.1".into()));
        assert!(broken(|r| r.paths.clear()));
        assert!(broken(|r| r.paths[0].prefix = "bot/".into()));
        assert!(broken(|r| r.paths[0].prefix = "bot{secret}".into()));
        assert!(broken(|r| r.paths[0].prefix = "../bot{secret}/".into()));
        assert!(broken(|r| r.paths[0].prefix = "{secret}/{secret}/".into()));
        assert!(broken(|r| r.methods = vec!["get".into()]));
        assert!(broken(|r| {
            r.header = Some(SecretHeader {
                name: "Authorization".into(),
                value: "Bearer {secret}".into(),
                basic: None,
            })
        }));
        let mut named = bot(TOKEN);
        named.name = "Bot".into();
        assert!(check(&named).is_err());
        // Never without a placeholder Link made: none, or one guessed,
        // would let anyone reaching the proxy have the secret added.
        for placeholder in ["", "123456:link-wire-", "123456:AAH-real_secret"] {
            let mut unplaced = bot(TOKEN);
            unplaced.placeholder = placeholder.to_string().into();
            assert!(check(&unplaced).is_err(), "{placeholder:?}");
        }
        assert!(check(&bot(" ")).is_err());
    }

    #[test]
    fn the_real_secret_goes_in_the_path() {
        for (target, sent) in [
            (
                "/_link/secret/bot/bot123456:link-wire/getUpdates?timeout=50",
                "/bot123456:AAH-real_secret/getUpdates?timeout=50",
            ),
            (
                "/_link/secret/bot/bot123456:link-wire/sendMessage",
                "/bot123456:AAH-real_secret/sendMessage",
            ),
            (
                "/_link/secret/bot/file/bot123456:link-wire/photos/file_1.jpg",
                "/file/bot123456:AAH-real_secret/photos/file_1.jpg",
            ),
        ] {
            assert_eq!(
                rewrite(&wired(target), &bot(TOKEN)).unwrap(),
                sent,
                "{target}"
            );
        }
    }

    #[test]
    fn what_would_send_the_bot_elsewhere_or_leave_its_path_is_refused() {
        for target in [
            "/_link/secret/bot/bot1:x/setWebhook?url=https://evil.example",
            "/_link/secret/bot/bot1:x/SETWEBHOOK",
            "/_link/secret/bot/bot1:x/logOut",
            "/_link/secret/bot/bot1:x/close",
            "/_link/secret/bot/bot1:x/../../evil",
            "/_link/secret/bot/bot1:x/send%4dessage",
            "/_link/secret/bot/bot1:x/send.Message",
            "/_link/secret/bot/bot1:x/setWebhook/",
            "/_link/secret/bot/bot1:x/",
            "/_link/secret/bot/bot/getMe",
            "/_link/secret/bot/getMe",
            "/_link/secret/bot/file/bot1:x/a/../../b",
            "/_link/secret/bot/file/bot1:x/a/./b",
            "/_link/secret/bot/file/bot1:x//etc/passwd",
            "/_link/secret/bot/file/bot1:x/a%2fb",
            "/_link/secret/bot/bot1:x@evil/getMe",
            "/_link/secret/other/bot1:x/getMe",
            "/_link/secret/bot",
            "/_link/model/v1/messages",
        ] {
            assert!(rewrite(&wired(target), &bot(TOKEN)).is_err(), "{target}");
        }
    }

    /// Only the program given the placeholder gets the secret added.
    #[test]
    fn only_the_placeholder_gets_the_secret() {
        for placeholder in [
            "123456:link-wire",
            "123456:AAH-real_secret",
            "1:x",
            &format!("{}x", bot_placeholder(TOKEN)),
            // Made for the same secret, by another run or holder.
            link_secret_contract::route::placeholder(
                &link_sandbox_route::Placeholders::new().unwrap(),
                "bot",
                &TOKEN.to_string().into(),
                Some(":"),
            )
            .expose(),
        ] {
            let target = format!("/_link/secret/bot/bot{placeholder}/getUpdates");
            let err = rewrite(&target, &bot(TOKEN)).unwrap_err();
            assert!(err.contains("placeholder"), "{placeholder}: {err}");
        }
    }

    #[test]
    fn a_head_gets_the_rules_host_and_loses_the_programs_own() {
        let head = b"POST /_link/secret/bot/bot1:link-wire/sendMessage HTTP/1.1\r\n\
            Host: 127.0.0.1:40781\r\nContent-Type: application/json\r\n\
            Content-Length: 2\r\nProxy-Authorization: x\r\nConnection: keep-alive\r\n\r\n";
        let head = wired(std::str::from_utf8(head).unwrap());
        let sent = String::from_utf8(prepare(head.as_bytes(), &bot(TOKEN)).unwrap()).unwrap();
        assert_eq!(
            sent,
            "POST /bot123456:AAH-real_secret/sendMessage HTTP/1.1\r\nHost: api.bots.example\r\n\
             Content-Type: application/json\r\nContent-Length: 2\r\n\r\n"
        );
        for bad in [
            &b"GET /_link/secret/bot/bot1:x/getMe HTTP/1.1\r\nX: a\nHost: evil\r\n\r\n"[..],
            b"DELETE /_link/secret/bot/bot1:x/getMe HTTP/1.1\r\n\r\n",
            b"GET /_link/secret/bot/bot1:x/getMe HTTP/2\r\n\r\n",
            b"GET /_link/secret/bot/bot1:x/getMe HTTP/1.1\r\nUpgrade: h2c\r\n\r\n",
        ] {
            let bad = wired(std::str::from_utf8(bad).unwrap());
            assert!(prepare(bad.as_bytes(), &bot(TOKEN)).is_err(), "{:?}", bad);
        }
    }

    #[test]
    fn a_secret_in_a_header_replaces_the_placeholders() {
        let secret = link_secret_contract::route::Sealed::new(
            &RUN,
            "api",
            "xoxb-real".to_string().into(),
            SecretRule {
                host: "api.example.com".into(),
                paths: vec![SecretPath {
                    prefix: "api/".into(),
                    rest: SecretRest::Name,
                }],
                header: Some(SecretHeader {
                    name: "Authorization".into(),
                    value: "Bearer {secret}".into(),
                    basic: None,
                }),
                refuse: vec!["api/revoke".into()],
                ..SecretRule::default()
            },
        );
        check(&secret).unwrap();
        let placeholder = secret.placeholder.expose().clone();
        let head = |path: &str, auth: &str| {
            format!("POST /_link/secret/api/{path} HTTP/1.1\r\nAuthorization: {auth}\r\n\r\n")
        };
        let sent = prepare(
            head("api/postMessage", &format!("Bearer {placeholder}")).as_bytes(),
            &secret,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(sent).unwrap(),
            "POST /api/postMessage HTTP/1.1\r\nHost: api.example.com\r\n\
             Authorization: Bearer xoxb-real\r\n\r\n"
        );
        for (path, auth) in [
            ("api/postMessage", "Bearer xoxb-guess"),
            ("api/postMessage", "Bearer xoxb-real"),
            ("api/REVOKE", &format!("Bearer {placeholder}")),
            ("other/postMessage", &format!("Bearer {placeholder}")),
        ] {
            assert!(
                prepare(head(path, auth).as_bytes(), &secret).is_err(),
                "{path} {auth}"
            );
        }
        let bare = "POST /_link/secret/api/api/postMessage HTTP/1.1\r\n\r\n";
        assert!(prepare(bare.as_bytes(), &secret).is_err());
    }

    /// A token git sends as HTTP Basic's credentials: the program's
    /// header is the base64 of `<user>:<placeholder>`, and what goes on
    /// is the base64 of `<user>:<token>`.
    #[test]
    fn a_basic_header_is_the_credentials_encoded() {
        use base64::Engine;
        let b64 = |text: String| base64::engine::general_purpose::STANDARD.encode(text);
        let secret = link_secret_contract::route::Sealed::new(
            &RUN,
            "github-token",
            "ghs_real".to_string().into(),
            SecretRule {
                host: "github.com".into(),
                paths: vec![SecretPath {
                    prefix: String::new(),
                    rest: SecretRest::Path,
                }],
                header: Some(SecretHeader {
                    name: "Authorization".into(),
                    value: "Basic {secret}".into(),
                    basic: Some("x-access-token".into()),
                }),
                ..SecretRule::default()
            },
        );
        check(&secret).unwrap();
        let placeholder = secret.placeholder.expose().clone();
        let head = |auth: &str| {
            format!(
                "GET /_link/secret/github-token/me/app.git/info/refs?service=git-upload-pack \
                 HTTP/1.1\r\nAuthorization: {auth}\r\n\r\n"
            )
        };
        let given = format!("Basic {}", b64(format!("x-access-token:{placeholder}")));
        let sent = String::from_utf8(prepare(head(&given).as_bytes(), &secret).unwrap()).unwrap();
        assert_eq!(
            sent,
            format!(
                "GET /me/app.git/info/refs?service=git-upload-pack HTTP/1.1\r\nHost: \
                 github.com\r\nAuthorization: Basic {}\r\n\r\n",
                b64("x-access-token:ghs_real".into())
            )
        );
        // The placeholder bare, or another user's, is refused.
        for auth in [
            format!("Basic {placeholder}"),
            format!("Basic {}", b64(format!("someone:{placeholder}"))),
        ] {
            assert!(prepare(head(&auth).as_bytes(), &secret).is_err(), "{auth}");
        }
        let mut odd = secret.rule.clone();
        odd.header.as_mut().unwrap().basic = Some("a:b".into());
        assert!(odd.check().is_err(), "a user holding `:`");
    }

    /// A sealed secret (`lnk secret create --internet-host`): any path of its one
    /// host, its header added for the program given the placeholder.
    #[test]
    fn a_sealed_secret_takes_any_path_of_its_host() {
        let secret = link_secret_contract::route::Sealed::new(
            &RUN,
            "stripe-key",
            "sk_live_real".to_string().into(),
            SecretRule {
                host: "api.stripe.com".into(),
                paths: vec![SecretPath {
                    prefix: String::new(),
                    rest: SecretRest::Path,
                }],
                header: Some(SecretHeader {
                    name: "Authorization".into(),
                    value: "Bearer {secret}".into(),
                    basic: None,
                }),
                methods: ["GET", "POST", "DELETE"].map(String::from).to_vec(),
                ..SecretRule::default()
            },
        );
        check(&secret).unwrap();
        let placeholder = secret.placeholder.expose().clone();
        let head = |method: &str, path: &str, auth: &str| {
            format!(
                "{method} /_link/secret/stripe-key/{path} HTTP/1.1\r\nAuthorization: {auth}\r\n\r\n"
            )
        };
        let sent = prepare(
            head(
                "DELETE",
                "v1/customers/cus_1?expand=x",
                &format!("Bearer {placeholder}"),
            )
            .as_bytes(),
            &secret,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(sent).unwrap(),
            "DELETE /v1/customers/cus_1?expand=x HTTP/1.1\r\nHost: api.stripe.com\r\n\
             Authorization: Bearer sk_live_real\r\n\r\n"
        );
        for (method, path, auth) in [
            ("GET", "v1/charges", "Bearer sk_live_guess"),
            ("GET", "v1/../charges", &format!("Bearer {placeholder}")),
            ("PUT", "v1/charges", &format!("Bearer {placeholder}")),
            ("GET", "", &format!("Bearer {placeholder}")),
        ] {
            assert!(
                prepare(head(method, path, auth).as_bytes(), &secret).is_err(),
                "{method} {path} {auth}"
            );
        }
    }

    /// A sealed secret granted to a service is read from what's granted
    /// at each call, as its caller's binding names that service: given
    /// while it's granted, refused once taken back, its caller and route
    /// unchanged; and never to a caller with no service, a person's run.
    #[test]
    fn a_grant_taken_back_is_refused_at_the_next_call() {
        use link_secret_contract::keeper::Request;
        use link_secret_contract::route::{Given, Kept, Named};
        let dir = link_test_kit::TempDir::new("granted");
        let store = link_secret_contract::Store::at(dir.path().join("secrets.toml"));
        let service = "s1s2s3s4s5s6";
        let grantee =
            link_secret_contract::Grantee::new("run:m1m2m3m4m5m6", "main", service).unwrap();
        let sealing =
            link_secret_contract::Sealing::new("api.bots.example", None, None, None).unwrap();
        let value: link_secretive::Secret = TOKEN.to_string().into();
        store
            .answer(
                Request::Add {
                    name: "BOT".into(),
                    value: value.duplicate(),
                    sealed: Some(sealing.clone()),
                    to: vec![grantee.clone()],
                },
                "",
            )
            .unwrap()
            .replaced()
            .unwrap();
        let given = Given {
            secrets: Vec::new(),
            granted: vec![Named::new(
                &RUN,
                "BOT",
                Kept::Variable("BOT".into()),
                &value,
                sealing.rule(),
            )],
        };
        let granted = Granted::Kept(store.clone());
        let call = |service: &str| {
            given.find(
                "/_link/secret/BOT/sendMessage",
                |which| granted.to(service, which),
                |_, _, _| anyhow::bail!("no token here"),
            )
        };
        let found = call(service).unwrap().expect("granted, so given");
        assert_eq!(found.value.expose(), TOKEN);
        assert!(call("").is_err(), "a caller with no service is given it");
        store
            .answer(
                Request::Grant {
                    called: link_secret_contract::Called::Name("BOT".into()),
                    add: Vec::new(),
                    remove: vec![grantee],
                },
                "",
            )
            .unwrap()
            .granted()
            .unwrap();
        let refused = format!("{:#}", call(service).unwrap_err());
        assert!(refused.contains("isn't granted"), "{refused}");
    }

    #[tokio::test]
    async fn a_call_reaches_the_service_with_the_secret_and_the_answer_comes_back() {
        let (mut program, proxy_end) = tokio::io::duplex(64 * 1024);
        let (service_end, mut service) = tokio::io::duplex(64 * 1024);
        let head = wired(
            "POST /_link/secret/bot/bot123456:link-wire/sendMessage HTTP/1.1\r\n\
             Content-Length: 11\r\n\r\n",
        );
        let relayed = tokio::spawn(async move {
            relay_to(
                link_sandbox_route::Unsaid(proxy_end),
                head.as_bytes(),
                b"{\"a\":".to_vec(),
                &bot(TOKEN),
                &Pool::new(),
                "direct",
                || async { Ok(service_end) },
            )
            .await
        });
        program.write_all(b"\"bcd\"}").await.unwrap();
        let mut got = vec![0u8; 4096];
        let mut seen = Vec::new();
        while !seen.ends_with(b"\"bcd\"}") {
            let n = service.read(&mut got).await.unwrap();
            assert!(n > 0);
            seen.extend_from_slice(&got[..n]);
        }
        let seen = String::from_utf8(seen).unwrap();
        assert!(
            seen.starts_with("POST /bot123456:AAH-real_secret/sendMessage HTTP/1.1\r\n"),
            "{seen}"
        );
        assert!(seen.ends_with("\r\n\r\n{\"a\":\"bcd\"}"), "{seen}");
        service
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}")
            .await
            .unwrap();
        drop(service);
        drop(program.shutdown().await);
        let mut answer = String::new();
        program.read_to_string(&mut answer).await.unwrap();
        assert!(answer.ends_with("{\"ok\":true}"), "{answer}");
        assert!(!answer.contains("AAH-real_secret"));
        let _ = relayed.await;
    }
}
