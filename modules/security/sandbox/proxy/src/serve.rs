//! One proxied connection: `CONNECT host:port`, a plain `http://`
//! request, or a route's.

use anyhow::{Context, bail, ensure};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::dial::dial;
use crate::filter::Filter;
use crate::http::{
    Browser, HEAD_TIMEOUT, RequestHead, from_a_browser, host_port, one_request, proxy_authorized,
    read_head, refusal, reply, too_many, unauthorized,
};
use crate::routes::{Client, behind_secret, logged};
use link_sandbox_route::io::Activity;
use link_sandbox_route::io::IDLE;
use link_sandbox_route::io::joined_until_idle;
use link_sandbox_route::io::until_idle;

/// `call`, ended if every wire is stopped while it runs.
pub async fn watched<F>(wire: Option<&'static crate::wire::Wire>, call: F) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<()>>,
{
    let Some(wire) = wire else {
        return call.await;
    };
    tokio::select! {
        biased;
        result = call => result,
        () = wire.until_stopped() => bail!("refused: the wire was stopped or paused"),
    }
}

/// One proxied connection: `CONNECT host:port`, or a plain `http://`
/// request forwarded as the origin expects it; logged on its wire, and
/// ended if every wire is stopped.
pub async fn proxy<S>(client: S, rules: Filter) -> anyhow::Result<()>
where
    S: Client + 'static,
{
    let mut call = crate::wire::Logged(crate::wire::Call::on(rules.wire));
    let client = crate::wire::Counted::new(client, &call);
    let result = watched(rules.wire, serve(client, rules, &mut call)).await;
    call.end(&result);
    result
}

/// `head` and `target` in origin-form, when `target` asks for Link's
/// prefix in absolute-form on a loopback host: the proxy's own routes.
/// None for any other target.
fn own_route(head: &[u8], target: &str) -> Option<(Vec<u8>, String)> {
    let after = target.strip_prefix("http://")?;
    let (authority, path) = after.split_at(after.find('/')?);
    let (host, _) = host_port(authority, Some(80))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(|c| c == '[' || c == ']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if !loopback || !(path.starts_with("/_link/") || path == "/_link") {
        return None;
    }
    let end = head.windows(2).position(|w| w == b"\r\n")?;
    let line = std::str::from_utf8(&head[..end]).ok()?;
    let mut out = line.replacen(target, path, 1).into_bytes();
    out.extend_from_slice(&head[end..]);
    Some((out, path.to_string()))
}

/// A 403 for a request that looks like a web page's: saying why when
/// its sender is told ([`Browser::told`]), else bare.
async fn refuse_browser<S: Client>(client: &mut S, browser: &Browser) {
    match browser.told() {
        Some(text) => crate::bridged::answer(client, "403 Forbidden", &text).await,
        None => reply(client, "403 Forbidden").await,
    }
}

/// [`proxy`]'s connection, saying in `call` what it asked for, and when
/// it's allowed.
async fn serve<S>(mut client: S, rules: Filter, call: &mut crate::wire::Call) -> anyhow::Result<()>
where
    S: Client + 'static,
{
    let (head, rest) = tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut client))
        .await
        .context("no request in time")??;
    let request = match RequestHead::parse(&head) {
        Ok(request) => request,
        Err(why) => {
            call.verb = Some("unknown");
            reply(&mut client, "400 Bad Request").await;
            bail!("refused: {why}");
        }
    };
    let (method, target) = (request.method.to_string(), request.target.to_string());
    let (name, to) = logged(&method, &target, rules.routes);
    (call.verb, call.to) = (Some(name), to);
    // What a web page can send is refused before it counts toward a pause
    // or the calls a minute: on macOS the port is on the machine's
    // loopback, which a page can send requests to, or reach through a
    // name it points there. A page sends only origin-form (`GET /x`),
    // never `CONNECT` or `http://...`, whose `Host` is the site asked for,
    // and whose `Origin` a browser in the sandbox sends.
    let origin_form = !method.eq_ignore_ascii_case("CONNECT") && !target.starts_with("http://");
    if origin_form && let Some(browser) = from_a_browser(&request) {
        // Logged without the page's `Origin` or `Host`: which sites the
        // user visits is none of the log's business.
        (call.verb, call.to, call.outsider) = (Some("browser"), String::new(), true);
        refuse_browser(&mut client, &browser).await;
        bail!("refused: {}", browser.why());
    }
    // A way out (`CONNECT`, `http://...`) only with the run's secret: on
    // this machine's loopback, a process that outlived an earlier run
    // could otherwise reach a later run's proxy at the same port. Before
    // it counts toward a pause or the calls a minute.
    if !origin_form
        && let Some(secret) = rules.proxy_secret
        && !proxy_authorized(&request, secret.expose())
    {
        call.outsider = true;
        unauthorized(&mut client).await;
        bail!("refused: without this run's proxy secret");
    }
    // The proxy itself, asked in absolute-form (`GET
    // http://127.0.0.1:<port>/_link/...`), as a client that sends every
    // request through its proxy asks for a route: served in origin-form,
    // as a server takes absolute-form (RFC 9112, 3.2.2). Only Link's
    // prefix on a loopback host, which nothing here reaches otherwise.
    let own = own_route(&head, &target);
    // A page is kept off them as in origin-form: a browser in the sandbox
    // that sends loopback through its proxy too sends its page's `Origin`.
    if own.is_some()
        && let Some(browser) = from_a_browser(&request)
    {
        (call.verb, call.to, call.outsider) = (Some("browser"), String::new(), true);
        refuse_browser(&mut client, &browser).await;
        bail!("refused: {}", browser.why());
    }
    let target = match &own {
        Some((_, path)) => {
            let (name, to) = logged(&method, path, rules.routes);
            (call.verb, call.to) = (Some(name), to);
            path.clone()
        }
        None => target,
    };
    if let Some(why) = rules.wire.and_then(|w| w.why_stopped()) {
        reply(&mut client, "403 Forbidden").await;
        bail!("refused: {why}");
    }
    if let Some((wait, why)) = rules.wire.and_then(|w| w.over_limit()) {
        call.limited = true;
        too_many(&mut client, wait).await;
        bail!("refused: {why}");
    }
    if method.eq_ignore_ascii_case("CONNECT") {
        let Some((host, port)) = host_port(&target, None) else {
            reply(&mut client, "400 Bad Request").await;
            bail!("refused: CONNECT to {target:?}");
        };
        // The sandbox's own address for one of its endpoints' names (its
        // DNS's, its `/etc/hosts`'): that name, checked as any.
        let host = match crate::dns::named(&host) {
            Some(name) => {
                call.to = format!("{name}:{port}");
                name
            }
            None => host,
        };
        let rules = match rules.at_port(&host, port, 443) {
            Ok(rules) => rules,
            Err(err) => {
                reply(&mut client, "403 Forbidden").await;
                return Err(err);
            }
        };
        let mut upstream = match dial(&host, port, rules).await {
            Ok(upstream) => upstream,
            Err(err) => {
                call.metadata = err.is::<crate::wire::Metadata>();
                reply(&mut client, refusal(&err)).await;
                return Err(err);
            }
        };
        client
            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .await?;
        // With a list of hosts, the TLS inside goes to the host checked:
        // a CDN routes by the ClientHello's name, and would take a listed
        // host's tunnel to another of its sites.
        let rest = match rules.hosts {
            Some(_) if port == 443 => {
                tokio::time::timeout(HEAD_TIMEOUT, hello_for(&mut client, rest, &host))
                    .await
                    .map_err(|_| {
                        anyhow::anyhow!("refused: no TLS ClientHello for {host} in time")
                    })??
            }
            _ => rest,
        };
        call.allowed();
        upstream.write_all(&rest).await?;
        joined_until_idle(client, upstream, IDLE).await?;
        return Ok(());
    }
    // Its routes, those that need it behind the proxy's URL secret: on
    // macOS any program of the machine reaches this port, and only the
    // sandboxed program was given the secret.
    let secret = rules.url_secret.map(|s| s.expose().as_str());
    let asked = own.as_ref().map_or(&head[..], |(head, _)| &head[..]);
    let (head, target, mount) = match behind_secret(asked, &target, rules.routes, secret) {
        Ok(unlocked) => unlocked,
        Err(err) => {
            reply(&mut client, "403 Forbidden").await;
            return Err(err);
        }
    };
    if let Some(route) = mount {
        let Ok(head) = String::from_utf8(head) else {
            reply(&mut client, "400 Bad Request").await;
            bail!("refused: a request head that isn't UTF-8");
        };
        // A body asked to wait for a go: the proxy says go itself, since a
        // bridge answers nothing before its verdict, which may need it.
        let (head, go) = without_expect(&head);
        if go && rest.is_empty() {
            client.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await?;
        }
        let to = link_sandbox_route::To::Request { head };
        return crate::bridged::serve(client, route, to, rest, rules, call).await;
    }
    // Link's own prefix, with no route there: off on this proxy.
    if target.starts_with("/_link/") || target == "/_link" {
        reply(&mut client, "403 Forbidden").await;
        bail!(
            "refused: nothing at {} on this sandbox's proxy",
            target.chars().take(40).collect::<String>()
        );
    }
    // A plain request: absolute-form to origin-form.
    let Some(after) = target.strip_prefix("http://") else {
        reply(&mut client, "400 Bad Request").await;
        bail!("refused: only http:// requests or CONNECT, not {target:?}");
    };
    let (authority, path) = match after.find('/') {
        Some(i) => (&after[..i], &after[i..]),
        None => (after, "/"),
    };
    let Some((host, port)) = host_port(authority, Some(80)) else {
        reply(&mut client, "400 Bad Request").await;
        bail!("refused: no host in {target:?}");
    };
    let rules = match rules.at_port(&host, port, 80) {
        Ok(rules) => rules,
        Err(err) => {
            reply(&mut client, "403 Forbidden").await;
            return Err(err);
        }
    };
    // One request, to the host checked: its `Host` is that host, never
    // the client's (a CDN routes port 80 by it, past a host list), its
    // body exactly as long as it says, and the connection closed after,
    // so a second request on it never goes unchecked.
    let head = match one_request(&request, path, &host, port) {
        Ok(head) => head,
        Err(err) => {
            reply(&mut client, "400 Bad Request").await;
            return Err(err);
        }
    };
    let upstream = match dial(&host, port, rules).await {
        Ok(upstream) => upstream,
        Err(err) => {
            call.metadata = err.is::<crate::wire::Metadata>();
            reply(&mut client, refusal(&err)).await;
            return Err(err);
        }
    };
    call.allowed();
    let (head, length) = head;
    // Its body: what came with the head, then the rest from the client,
    // never more. The answer goes back until the origin closes, or
    // nothing crosses for a while, as for a tunnel.
    let early = rest.len().min(length as usize);
    let (mut from_client, mut to_client) = tokio::io::split(client);
    let (mut from_upstream, mut to_upstream) = tokio::io::split(upstream);
    let activity = Activity::new();
    let relayed = async {
        let mut to_upstream = activity.watch(&mut to_upstream);
        to_upstream.write_all(head.as_bytes()).await?;
        to_upstream.write_all(&rest[..early]).await?;
        let body = async {
            let mut body = activity.watch(&mut from_client).take(length - early as u64);
            tokio::io::copy(&mut body, &mut to_upstream).await
        };
        let (mut from_upstream, mut to_client) = (
            activity.watch(&mut from_upstream),
            activity.watch(&mut to_client),
        );
        let answer = tokio::io::copy(&mut from_upstream, &mut to_client);
        let (sent, answered) = tokio::join!(body, answer);
        sent?;
        answered?;
        Ok(())
    };
    until_idle(&activity, relayed).await?;
    to_client.shutdown().await?;
    Ok(())
}

/// `head` without its `Expect: 100-continue`, and whether it had one
/// (and a body it waits to send).
fn without_expect(head: &str) -> (String, bool) {
    let Ok(request) = RequestHead::parse(head.as_bytes()) else {
        return (head.to_string(), false);
    };
    if !request.expects_continue() || request.length().is_ok_and(|n| n == 0) {
        return (head.to_string(), false);
    }
    let mut out = String::with_capacity(head.len());
    for (i, line) in head.split_inclusive("\r\n").enumerate() {
        let expect = line
            .split_once(':')
            .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case("expect"));
        if i == 0 || !expect {
            out.push_str(line);
        }
    }
    (out, true)
}

/// The most a tunnel's TLS ClientHello is read up to.
const MAX_HELLO: usize = 64 * 1024;

/// The TLS ClientHello a tunnel to `host` starts with, read whole after
/// `read` (what came with the `CONNECT`): refused unless the name it
/// asks for (its SNI) is `host`. Everything read, to send on.
pub(crate) async fn hello_for<S>(
    client: &mut S,
    mut read: Vec<u8>,
    host: &str,
) -> anyhow::Result<Vec<u8>>
where
    S: tokio::io::AsyncRead + Unpin,
{
    let mut acceptor = rustls::server::Acceptor::default();
    let mut fed = 0;
    loop {
        while fed < read.len() {
            match acceptor.read_tls(&mut &read[fed..]) {
                Ok(n) if n > 0 => fed += n,
                _ => bail!("refused: a tunnel to {host} that isn't TLS"),
            }
        }
        match acceptor.accept() {
            Ok(Some(hello)) => {
                let name = hello.client_hello().server_name().map(str::to_string);
                let host = host.trim_end_matches('.');
                return match name {
                    Some(name) if name.trim_end_matches('.').eq_ignore_ascii_case(host) => Ok(read),
                    Some(name) => bail!("refused: a tunnel to {host} asking TLS for {name}"),
                    // An address listed: TLS names none for one (RFC 6066),
                    // and the tunnel goes to that address alone.
                    None if host
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .parse::<std::net::IpAddr>()
                        .is_ok() =>
                    {
                        Ok(read)
                    }
                    None => bail!("refused: a tunnel to {host} whose TLS names no host"),
                };
            }
            Ok(None) => {}
            Err(_) => bail!("refused: a tunnel to {host} that isn't TLS"),
        }
        ensure!(
            read.len() < MAX_HELLO,
            "refused: a tunnel to {host} whose TLS ClientHello is too long"
        );
        let mut more = [0u8; 4096];
        let n = client.read(&mut more).await?;
        ensure!(
            n > 0,
            "the tunnel to {host} closed before its TLS ClientHello"
        );
        read.extend_from_slice(&more[..n]);
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{own_route, proxy};
    use crate::filter::Filter;
    use link_sandbox_contract::net::Upstream;

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    /// What the proxy with `rules` answers `request` first.
    async fn answer(rules: Filter, request: &str) -> String {
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        tokio::spawn(proxy(server, rules));
        client.write_all(request.as_bytes()).await.unwrap();
        let mut got = [0u8; 4096];
        let n = tokio::time::timeout(std::time::Duration::from_secs(5), client.read(&mut got))
            .await
            .unwrap()
            .unwrap();
        String::from_utf8_lossy(&got[..n]).to_string()
    }

    fn basic(pair: &str) -> String {
        use base64::Engine;
        let token = base64::engine::general_purpose::STANDARD.encode(pair);
        format!("Proxy-Authorization: Basic {token}\r\n")
    }

    /// A request for the proxy's own prefix in absolute-form, as a client
    /// sending everything through its proxy makes one, is its route in
    /// origin-form, its headers kept; any other host or path is left as it is.
    #[test]
    fn its_own_prefix_asked_through_it_is_its_route() {
        let head = |target: &str| format!("GET {target} HTTP/1.1\r\nHost: x\r\nA: b\r\n\r\n");
        for (asked, path) in [
            (
                "http://127.0.0.1:40781/_link/secret/telegram/bot1:x/getMe",
                "/_link/secret/telegram/bot1:x/getMe",
            ),
            ("http://localhost:40781/_link/model", "/_link/model"),
            (
                "http://[::1]:40781/_link/url/k/here/1",
                "/_link/url/k/here/1",
            ),
        ] {
            let (rewritten, target) = own_route(head(asked).as_bytes(), asked).expect(asked);
            assert_eq!(target, path);
            assert_eq!(String::from_utf8(rewritten).unwrap(), head(path));
        }
        for left in [
            "http://example.com/_link/model",
            "http://10.0.0.1:40781/_link/model",
            "http://127.0.0.1:40781/other",
            "http://127.0.0.1:40781/_linkx",
            "/_link/model",
        ] {
            assert!(own_route(head(left).as_bytes(), left).is_none(), "{left}");
        }
    }

    /// Asked for in absolute-form, its own prefix still needs the run's
    /// secret, as every absolute-form request does.
    #[tokio::test]
    async fn its_own_prefix_asked_through_it_needs_the_runs_secret() {
        let rules =
            Filter::new(Upstream::Direct).with_proxy_secret(Some(SECRET.to_string().into()));
        let asked = "GET http://127.0.0.1:1/_link/model HTTP/1.1\r\nHost: 127.0.0.1:1\r\n\r\n";
        assert!(answer(rules, asked).await.starts_with("HTTP/1.1 407"));
    }

    /// Where only its sandbox reaches it, with no secret, a page's request
    /// for its own prefix in absolute-form is refused as in origin-form.
    #[tokio::test]
    async fn its_own_prefix_asked_through_it_from_a_page_is_refused() {
        let page = "GET http://127.0.0.1:1/_link/model HTTP/1.1\r\nHost: 127.0.0.1:1\r\n\
                    Origin: https://evil.example\r\n\r\n";
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let served = tokio::spawn(async move {
            let mut call = crate::wire::Call::on(None);
            let result = super::serve(server, Filter::new(Upstream::Direct), &mut call).await;
            (result.is_err(), call.verb)
        });
        client.write_all(page.as_bytes()).await.unwrap();
        let mut got = [0u8; 256];
        let n = client.read(&mut got).await.unwrap();
        assert!(String::from_utf8_lossy(&got[..n]).starts_with("HTTP/1.1 403"));
        assert_eq!(served.await.unwrap(), (true, Some("browser")));
    }

    /// A request on the proxy naming another host's `Host`, as a program
    /// curling a sealed secret's address with that host set does, is
    /// refused saying which host and why; a page's `Origin` still gets a
    /// bare 403.
    #[tokio::test]
    async fn another_hosts_host_is_refused_saying_why() {
        let asked = "GET /service/s1s2s3s4s5s6/headers HTTP/1.1\r\n\
                     Host: postman-echo.com\r\n\r\n";
        let refused = answer(Filter::new(Upstream::Direct), asked).await;
        assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
        let (_, body) = refused.split_once("\r\n\r\n").unwrap();
        assert!(body.starts_with("Link's sandbox: "), "{body}");
        assert!(body.contains("\"postman-echo.com\""), "{body}");
        assert!(
            body.contains("a sealed route serves only its own host"),
            "{body}"
        );
        let page = "GET /service/s1s2s3s4s5s6/headers HTTP/1.1\r\nHost: 127.0.0.1:1\r\n\
                    Origin: https://evil.example\r\n\r\n";
        let bare = answer(Filter::new(Upstream::Direct), page).await;
        assert!(bare.starts_with("HTTP/1.1 403"), "{bare}");
        assert!(!bare.contains("evil.example"), "{bare}");
        assert!(!bare.contains("Link's sandbox"), "{bare}");
    }

    /// A proxy on this machine's loopback takes a way out only with its
    /// run's secret: none, or another run's, is a 407 that says how to
    /// send it, before its rules are read; with it, its rules decide.
    /// Its routes, origin-form, need none.
    #[tokio::test]
    async fn a_way_out_needs_the_runs_secret() {
        let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = target.local_addr().unwrap();
        tokio::spawn(async move { while target.accept().await.is_ok() {} });
        let rules = Filter {
            allowed: |_| true,
            ..Filter::new(Upstream::Direct)
        }
        .with_tcp(vec![at.to_string()])
        .with_proxy_secret(Some(SECRET.to_string().into()));
        let connect = |auth: &str| format!("CONNECT {at} HTTP/1.1\r\nHost: {at}\r\n{auth}\r\n");
        for auth in [
            String::new(),
            basic("lnk:an0ther0runs0secret0000000000"),
            basic(SECRET),
            format!("Proxy-Authorization: Bearer {SECRET}\r\n"),
        ] {
            let refused = answer(rules, &connect(&auth)).await;
            assert!(refused.starts_with("HTTP/1.1 407"), "{auth}: {refused}");
            assert!(refused.contains("Proxy-Authenticate: Basic"), "{refused}");
        }
        let plain = answer(
            rules,
            "GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n",
        );
        assert!(plain.await.starts_with("HTTP/1.1 407"));
        for user in ["lnk", "anyone", ""] {
            let taken = answer(rules, &connect(&basic(&format!("{user}:{SECRET}")))).await;
            assert!(taken.starts_with("HTTP/1.1 200"), "{user}: {taken}");
        }
        // Its own prefix, origin-form: its route rules answer, not 407.
        let route = answer(
            rules,
            "GET /_link/model HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        )
        .await;
        assert!(route.starts_with("HTTP/1.1 403"), "{route}");
        // A proxy only its sandbox reaches takes it without.
        let inside = Filter {
            allowed: |_| true,
            ..Filter::new(Upstream::Direct)
        }
        .with_tcp(vec![at.to_string()]);
        let taken = answer(inside, &connect("")).await;
        assert!(taken.starts_with("HTTP/1.1 200"), "{taken}");
    }
}
