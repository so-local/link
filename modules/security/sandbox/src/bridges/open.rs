//! The open route: a page opened in your browser, outside, for a program
//! in the sandbox, after asking you each time; served by the sandbox's
//! bridge ([`crate::bridges`]).
//!
//! A program asks its proxy with a `POST` to `$LNK_SANDBOX_OPEN`
//! (`/_link/url/<secret>/open`, behind the proxy's URL secret, since on macOS
//! any program of the machine reaches its port), the page's URL as its
//! body, which the proxy hands here;
//! in the sandbox, `$BROWSER` is `lnk-sandbox` itself, which does that
//! with the URL it's given ([`request`]), so Python's
//! `webbrowser`, `gh` and `xdg-open` all reach it. Only `https://` pages,
//! in plain ASCII and never this machine or its network by address. It
//! opens in *your* browser, with your cookies and logins, so every page
//! asks you and shows its whole URL: no host is ever allowed in advance.
//!
//! A login that answers on this machine's loopback (OAuth's
//! `redirect_uri=http://localhost:<port>/...`) gets that port for its
//! length. On macOS the sandbox shares this machine's loopback, and the
//! program listens there itself. On Linux its loopback is its own: the
//! bridge listens on that port here and passes the browser's connection
//! in, through the socket into the sandbox's network its proxy names
//! ([`link_sandbox_route::Call::inbound`]), until the first request
//! arrives or five minutes pass, one port for each page opened, logged
//! on the sandbox's wire as `callback`.

mod callback;
mod request;

use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, ensure};
use link_format::WithinExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use link_sandbox_client::grants::Authority;
use link_sandbox_route::Inbound;
use link_sandbox_route::Verdict;
use link_sandbox_route::Verdicts;
use link_sandbox_route::http::RequestHead;

pub use callback::CALLBACK_FOR;
use callback::{callback, listen};
pub use request::{is_page, request};

use link_sandbox_client::routes::OPEN as PREFIX;

/// The longest URL opened: each character of it is shown when it asks.
pub const MAX_URL: usize = 2048;
/// How long a request's body may take to arrive.
const BODY_TIMEOUT: Duration = Duration::from_secs(30);

/// What opens pages for one sandbox: who asks, and how.
#[derive(Clone, Debug)]
pub struct Opener {
    /// The sandbox's name, as a question names it.
    pub sandbox: String,
    /// Link's `sandbox/` folder, where a question waits ([`crate::asks`]).
    pub dir: PathBuf,
    /// The command that opens a page, given it last; None: nothing opens
    /// one here (a box, with no desktop).
    pub launcher: Option<Vec<String>>,
    /// Linux: the socket into the sandbox's own network, which a login's
    /// callback port is passed in through. None: the sandbox listens on
    /// this machine's loopback itself (macOS).
    pub inbound: Option<Inbound>,
    /// The only hosts a page may be on; `["*"]`, any.
    pub hosts: Vec<String>,
    /// Your desktop session's variables, which opening a page needs.
    pub desktop: Vec<(String, String)>,
    /// Whom its questions are asked for, and how long each waits.
    pub asking: Option<link_sandbox_client::routes::Asking>,
}

impl Opener {
    /// This machine's way of opening a page for `sandbox`, by the open
    /// route's `given`: `open` on macOS, `xdg-open` on a Linux desktop,
    /// none without one.
    pub fn here(
        sandbox: &str,
        dir: PathBuf,
        inbound: Option<Inbound>,
        given: link_sandbox_client::routes::Open,
    ) -> Self {
        let desktop = link_sandbox_route::desktop::only_desktop(&given.desktop);
        let shown = desktop
            .iter()
            .any(|(k, v)| (k == "DISPLAY" || k == "WAYLAND_DISPLAY") && !v.is_empty());
        let launcher = if cfg!(target_os = "macos") {
            Some(vec!["/usr/bin/open".to_string()])
        } else {
            shown.then(|| vec!["xdg-open".to_string()])
        };
        Self {
            sandbox: sandbox.into(),
            dir,
            launcher,
            inbound,
            hosts: given.hosts,
            desktop,
            asking: given.asking,
        }
    }
}

/// A page to open: its host, and the port its login answers on here, if
/// it's one that does.
#[derive(Debug, PartialEq)]
pub struct Page {
    pub host: String,
    pub callback: Option<u16>,
}

/// `url` as a page to open, or why it isn't one: `https://`, at most
/// [`MAX_URL`] characters of plain ASCII with no spaces (so what's shown
/// is what opens), no user in its address (`https://bank.com@evil.com`),
/// and never this machine or its network by address.
pub fn check_url(url: &str) -> Result<Page, String> {
    if url.len() > MAX_URL {
        return Err(format!("a URL over {MAX_URL} characters"));
    }
    if !url.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(
            "a URL with a space, a control character or a character that isn't plain ASCII \
             (a name in another script is written in its xn-- form)"
                .into(),
        );
    }
    let Some(after) = url.strip_prefix("https://") else {
        return Err("only https:// pages open".into());
    };
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let authority = &after[..end];
    if authority.contains('@') {
        return Err("a URL with a user in its address".into());
    }
    let Some((host, _)) = link_sandbox_route::http::host_port(authority, Some(443)) else {
        return Err(format!("no host in {authority:?}"));
    };
    let host = host.to_ascii_lowercase();
    // The address a browser takes it for: IPv6 in brackets, IPv4 in any
    // form a URL may write it (`127.1`, `2130706433`, `0x7f000001`).
    let ip = if authority.starts_with('[') {
        let Ok(v6) = host.parse::<std::net::Ipv6Addr>() else {
            return Err(format!("{host} isn't an IPv6 address"));
        };
        Some(IpAddr::V6(v6))
    } else if link_sandbox_route::http::ends_in_number(&host) {
        let Some(v4) = link_sandbox_route::http::url_ipv4(&host) else {
            return Err(format!("{host} is neither a name nor an address"));
        };
        Some(IpAddr::V4(v4))
    } else {
        None
    };
    let name = host.trim_end_matches('.');
    let local = name == "localhost"
        || name.ends_with(".localhost")
        || ip.is_some_and(|ip| {
            link_sandbox_contract::net::forbidden(ip)
                || link_sandbox_contract::net::this_machine(ip)
        });
    if local {
        return Err(format!("{host} is this machine or its network"));
    }
    Ok(Page {
        host,
        callback: callback_port(url),
    })
}

/// Why `host`, a page's name, is refused: it resolves (within a few
/// seconds) to an address of this machine, its network or a metadata
/// service. None for an address, which [`check_url`] judged, and for a
/// name that doesn't resolve.
async fn resolves_here(host: &str) -> Option<String> {
    let name = host.trim_start_matches('[').trim_end_matches(']');
    if name.parse::<IpAddr>().is_ok() || link_sandbox_route::http::ends_in_number(name) {
        return None;
    }
    let found = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        link_sandbox_route::resolve::resolve(name, 443),
    )
    .await;
    let Ok(Ok(addrs)) = found else {
        return None;
    };
    addrs
        .iter()
        .any(|a| {
            link_sandbox_contract::net::forbidden(a.ip())
                || link_sandbox_contract::net::this_machine(a.ip())
        })
        .then(|| format!("{host} resolves to this machine or its network"))
}

/// The port on this machine's loopback a login answers on: its
/// `redirect_uri`, if that's `http://localhost:<port>` (or `127.0.0.1`,
/// `[::1]`) with a port from 1024 (a sandbox can't listen below it).
fn callback_port(url: &str) -> Option<u16> {
    let query = url.split('#').next()?.split_once('?')?.1;
    let value = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("redirect_uri="))?;
    let redirect = percent_decoded(value)?;
    let rest = ["http://localhost:", "http://127.0.0.1:", "http://[::1]:"]
        .iter()
        .find_map(|prefix| redirect.strip_prefix(prefix))?;
    let digits = rest.split(['/', '?', '#']).next()?;
    let port: u16 = digits.parse().ok()?;
    (port >= 1024).then_some(port)
}

/// `text` with its `%XX` decoded; None if one is broken or it isn't text.
fn percent_decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What a page asks you: the whole URL, and what opening it means.
pub fn question(
    sandbox: &str,
    url: &str,
    page: &Page,
    served: bool,
) -> crate::bridges::asks::Proposal {
    let mut details = vec![
        url.to_string(),
        "It opens with your logins and cookies: allow it only if you asked for this.".to_string(),
    ];
    if let (Some(port), true) = (page.callback, served) {
        details.push(format!(
            "Its login answers on localhost:{port}, which Link passes into the sandbox until \
             it does, for five minutes at most."
        ));
    }
    let mut proposal = crate::bridges::asks::Proposal::sandbox(
        sandbox,
        format!("open a page on {} in your browser", page.host),
        details,
    );
    proposal.payload = url.as_bytes().to_vec();
    proposal
}

/// One call on the route (the head already read, and `rest` of what
/// came after it): one page, asked about, opened, answered, closed. Its
/// verdict says the page's host; a login's callback is logged on `wire`,
/// the sandbox's.
pub async fn serve<S>(
    mut client: S,
    head: &[u8],
    rest: Vec<u8>,
    opener: &Opener,
    wire: Option<link_sandbox_proxy::wire::Wire>,
    authority: &Authority,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Verdicts + Unpin,
{
    let mut call = Verdict::default();
    let call = &mut call;
    let length = match parse(head) {
        Ok(length) => length,
        Err(why) => return refuse(&mut client, call, "400 Bad Request", &why).await,
    };
    let body = match tokio::time::timeout(BODY_TIMEOUT, read_body(&mut client, rest, length)).await
    {
        Ok(Ok(body)) => body,
        _ => {
            return refuse(
                &mut client,
                call,
                "400 Bad Request",
                "the page's URL didn't arrive whole",
            )
            .await;
        }
    };
    let url = String::from_utf8_lossy(&body).trim().to_string();
    let page = match check_url(&url) {
        Ok(page) => page,
        Err(why) => return refuse(&mut client, call, "403 Forbidden", &why).await,
    };
    call.to = page.host.clone();
    let allowed = authority
        .page_hosts(&opener.hosts)
        .is_some_and(|lists| link_sandbox_client::grants::page_allowed(&page.host, &lists));
    if !allowed {
        let why = format!(
            "{} isn't one of the hosts this sandbox may reach",
            page.host
        );
        return refuse(&mut client, call, "403 Forbidden", &why).await;
    }
    // A name that resolves here to this machine or its network
    // (`127.0.0.1.nip.io`) is refused as its address is. What the browser
    // resolves it to later is its own lookup.
    if let Some(why) = resolves_here(&page.host).await {
        return refuse(&mut client, call, "403 Forbidden", &why).await;
    }
    let Some(launcher) = opener.launcher.clone() else {
        return refuse(
            &mut client,
            call,
            "403 Forbidden",
            "there's no desktop here to open a page on",
        )
        .await;
    };
    let served = page.callback.is_some() && opener.inbound.is_some();
    let asking = opener.asking.as_ref();
    let question =
        question(&opener.sandbox, &url, &page, served).for_agent(asking.map(|a| a.agent.as_str()));
    let wait = crate::bridges::asks::wait_of(asking);
    let (dir, desktop) = (opener.dir.clone(), opener.desktop.clone());
    let answer = tokio::task::spawn_blocking(move || {
        crate::bridges::asks::ask(
            &dir,
            &question,
            &desktop,
            crate::bridges::asks::Waits::Always,
            wait,
        )
    })
    .await;
    let mut reviewed = None;
    let why = match answer {
        Ok(Ok(got)) if got.answer == crate::bridges::asks::Answer::Allowed => {
            reviewed = Some(got);
            None
        }
        Ok(Ok(got)) if got.answer == crate::bridges::asks::Answer::Refused => Some(got.refusal()),
        Ok(Ok(_)) => Some(format!(
            "opening a page asks you first, and no one answered in {} seconds (lnk sandbox \
             asks shows what's waiting)",
            wait.as_secs()
        )),
        Ok(Err(err)) => Some(format!(
            "opening a page asks you first, and asking failed: {err:#}"
        )),
        Err(_) => Some("asking you failed".into()),
    };
    if let Some(why) = why {
        return refuse(&mut client, call, "403 Forbidden", &why).await;
    }
    // Its login's port, listened for before the browser can answer on it.
    // Taken here by another program, the answer would go there: refused.
    let listeners = match (page.callback, &opener.inbound) {
        (Some(port), Some(_)) => match listen(port).await {
            Ok(listeners) => listeners,
            Err(err) => {
                let why = format!(
                    "its login answers on localhost:{port}, which another program here \
                     listens on: {err:#}"
                );
                return refuse(&mut client, call, "403 Forbidden", &why).await;
            }
        },
        _ => Vec::new(),
    };
    let live = !link_sandbox_proxy::wire::is_stopped(&opener.dir)
        && !link_sandbox_proxy::wire::paused(&opener.dir)
            .iter()
            .any(|(name, _)| name == &opener.sandbox);
    let application = reviewed
        .expect("allowed review")
        .claim(url.as_bytes(), live)?;
    if let Err(err) = launch(&launcher, &opener.desktop, &url).await {
        application.report(link_interactions::approval::EffectOutcome::Failed {
            reason: format!("{err:#}"),
        })?;
        call.outcome = link_sandbox_route::Outcome::Failed;
        client.say(call).await?;
        let text = format!("Link's sandbox: couldn't open it here: {err:#}\n");
        let _ = respond(&mut client, "502 Bad Gateway", &text).await;
        return Err(err).within("opening the page");
    }
    application.report(link_interactions::approval::EffectOutcome::Applied {
        detail: "The reviewed URL was passed to the browser launcher.".into(),
    })?;
    client.say(call).await?;
    if let (Some(port), Some(inbound)) = (page.callback, &opener.inbound)
        && !listeners.is_empty()
    {
        // Logged as long as it's served: the process's own.
        let wire = wire.map(|wire| &*Box::leak(Box::new(wire)));
        tokio::spawn(callback(listeners, inbound.clone(), port, wire));
    }
    respond(&mut client, "200 OK", "Opened in your browser.\n").await
}

/// The request's body length: a `POST` to the verb, with a length and no
/// more than a URL's.
fn parse(head: &[u8]) -> Result<usize, String> {
    let request = RequestHead::parse(head)?;
    if !request.method.eq_ignore_ascii_case("POST") {
        return Err(format!("{PREFIX} takes a POST, the page's URL as its body"));
    }
    if request.target.trim_end_matches('/') != PREFIX {
        return Err(format!("{} isn't {PREFIX}", request.target));
    }
    if !request.headers.iter().any(|h| h.name == "content-length") {
        return Err("a body without a length".into());
    }
    let length = request.length()? as usize;
    // The URL, and a line end or two.
    if length > MAX_URL + 2 {
        return Err(format!("a URL over {MAX_URL} characters"));
    }
    Ok(length)
}

/// `length` bytes of body: `rest` and what follows it.
async fn read_body<S: AsyncRead + Unpin>(
    client: &mut S,
    mut body: Vec<u8>,
    length: usize,
) -> anyhow::Result<Vec<u8>> {
    ensure!(
        body.len() <= length,
        "more than one request on the connection"
    );
    let mut chunk = [0u8; 4096];
    while body.len() < length {
        let want = (length - body.len()).min(chunk.len());
        let n = client.read(&mut chunk[..want]).await?;
        ensure!(n > 0, "the connection closed mid-body");
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(body)
}

/// Opens `url` with `launcher`, found on the system's `PATH`, with your
/// desktop session's variables and none of the program's (its proxy, a
/// harness's HOME). A launcher still
/// running after a few seconds is taken to have opened it (a browser
/// started in its place) and left to run.
async fn launch(
    launcher: &[String],
    desktop: &[(String, String)],
    url: &str,
) -> anyhow::Result<()> {
    let (program, args) = launcher.split_first().within("nothing opens pages here")?;
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .arg(url)
        .env_clear()
        .env("PATH", link_sandbox_route::desktop::SYSTEM_PATH)
        .envs(desktop.iter().cloned())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .within(format!("running {program}"))?;
    match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
        Ok(status) => {
            let status = status?;
            ensure!(status.success(), "{program} failed ({status})");
            Ok(())
        }
        Err(_) => Ok(()),
    }
}

async fn respond<S: AsyncWrite + Unpin>(
    client: &mut S,
    status: &str,
    text: &str,
) -> anyhow::Result<()> {
    client
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{text}",
                text.len()
            )
            .as_bytes(),
        )
        .await?;
    client.shutdown().await?;
    Ok(())
}

/// Refuses the call, `why`: its verdict `call`, refused, then `status`.
async fn refuse<S: AsyncWrite + Verdicts + Unpin>(
    client: &mut S,
    call: &mut Verdict,
    status: &str,
    why: &str,
) -> anyhow::Result<()> {
    call.outcome = link_sandbox_route::Outcome::Refused;
    client.say(call).await?;
    let _ = respond(client, status, &format!("Link's sandbox: {why}\n")).await;
    bail!("refused: {why}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_https_pages_elsewhere_open() {
        let page = check_url("https://github.com/login/device").unwrap();
        assert_eq!(page.host, "github.com");
        assert_eq!(page.callback, None);
        for bad in [
            "http://example.com/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://example.com/a b",
            "https://example.com/\x1b[2J",
            "https://exämple.com/",
            "https://bank.com@evil.com/",
            "https://localhost/",
            "https://app.localhost:3000/",
            "https://127.0.0.1/",
            "https://192.168.1.1/",
            "https://[::1]/",
            "https://169.254.169.254/latest/meta-data/",
            "https://",
            "https:///path",
            // This machine and its network as a browser reads them.
            "https://127.1/",
            "https://2130706433/",
            "https://0x7f000001/",
            "https://0177.0.0.1/",
            "https://127.0.0.1./",
            "https://0/",
            "https://0x/",
            "https://3232235777/admin",
            "https://192.168.257/",
            "https://0xa9.0xfe.0xa9.0xfe/",
            "https://localhost./",
            "https://LOCALHOST../",
            "https://app.localhost.:3000/",
            // What a browser refuses as an address.
            "https://1.2.3.4.5/",
            "https://256.1.1.1/",
            "https://example.09/",
            "https://[localhost]/",
        ] {
            assert!(check_url(bad).is_err(), "{bad:?}");
        }
        for good in [
            "https://8.8.8.8/",
            "https://134744072/",
            "https://example.com./",
        ] {
            assert!(check_url(good).is_ok(), "{good:?}");
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL));
        assert!(check_url(&long).is_err());
    }

    #[test]
    fn a_logins_loopback_callback_is_found() {
        let port = |url: &str| check_url(url).unwrap().callback;
        assert_eq!(
            port(
                "https://accounts.example.com/o/oauth2/auth?client_id=x&redirect_uri=\
                 http%3A%2F%2Flocalhost%3A8085%2Fcallback&state=y"
            ),
            Some(8085)
        );
        assert_eq!(
            port("https://id.example.com/authorize?redirect_uri=http://127.0.0.1:53682/"),
            Some(53682)
        );
        assert_eq!(
            port("https://id.example.com/a?redirect_uri=http%3A%2F%2F%5B%3A%3A1%5D%3A4000"),
            Some(4000)
        );
        for none in [
            // Not this machine's loopback, below 1024, a look-alike, broken.
            "https://id.example.com/a?redirect_uri=https%3A%2F%2Fapp.example.com%2Fcb",
            "https://id.example.com/a?redirect_uri=http%3A%2F%2Flocalhost%3A80%2F",
            "https://id.example.com/a?redirect_uri=http%3A%2F%2Flocalhost.evil.com%3A8080%2F",
            "https://id.example.com/a?redirect_uri=http%3A%2F%2Flocalhost%3A8080x",
            "https://id.example.com/a?redirect_uri=%zz",
            "https://id.example.com/a?next=http%3A%2F%2Flocalhost%3A8080%2F",
            "https://id.example.com/a#redirect_uri=http%3A%2F%2Flocalhost%3A8080%2F",
        ] {
            assert_eq!(port(none), None, "{none}");
        }
    }

    #[tokio::test]
    async fn a_name_resolving_to_this_machine_is_refused() {
        // Resolved here, as `127.0.0.1.nip.io` would be.
        assert!(resolves_here("localhost").await.is_some());
        // An address is check_url's to judge; a name that resolves
        // nowhere opens nothing here.
        assert!(resolves_here("1.1.1.1").await.is_none());
        assert!(resolves_here("[::1]").await.is_none());
        assert!(resolves_here("no-such-name.invalid").await.is_none());
    }

    fn scratch(name: &str) -> (link_test_kit::TempDir, std::path::PathBuf) {
        let root = link_test_kit::TempDir::new(&format!("open-{name}"));
        let dir = link_files::layout::Hidden::Sandboxes.in_home(&root);
        link_files::create_private_dir(&dir).unwrap();
        (root, dir)
    }

    /// Answers the next question waiting in `dir`, `yes`, from a thread.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn answering(dir: &std::path::Path, yes: bool) -> std::thread::JoinHandle<bool> {
        let dir = dir.to_path_buf();
        // Within 30 s: other tests' questions take turns before it.
        std::thread::spawn(move || {
            for _ in 0..1500 {
                if let Some(ask) = crate::bridges::asks::waiting(&dir).unwrap().first() {
                    assert!(ask.question.title.contains("open a page on id.example.com"));
                    return crate::bridges::asks::answer(&dir, &ask.response_code, yes).unwrap();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        })
    }

    /// `url` asked for with `opener`, as its route's call: its answer.
    async fn opened(opener: &Opener, url: &str) -> String {
        opened_as(opener, url, Authority::Run).await
    }

    /// `url` asked for with `opener` by a caller of `authority`.
    async fn opened_as(opener: &Opener, url: &str, authority: Authority) -> String {
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let request = format!(
            "POST {PREFIX} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n",
            url.len()
        );
        let opener = opener.clone();
        let serving = tokio::spawn(async move {
            let server = link_sandbox_route::Unsaid(server);
            serve(
                server,
                request.as_bytes(),
                Vec::new(),
                &opener,
                None,
                &authority,
            )
            .await
        });
        client.write_all(url.as_bytes()).await.unwrap();
        let mut answer = String::new();
        client.read_to_string(&mut answer).await.unwrap();
        let _ = serving.await;
        answer
    }

    #[tokio::test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    async fn a_page_opens_once_you_allow_it_and_its_login_answers_inside() {
        let (_root, dir) = scratch("flow");
        // The sandbox's side of the sockets: what arrives for its port.
        let inbound = dir.join("in.sock");
        let sandbox = tokio::net::UnixListener::bind(&inbound).unwrap();
        tokio::spawn(async move {
            while let Ok((mut link, _)) = sandbox.accept().await {
                let port = link.read_u16().await.unwrap();
                let mut request = [0u8; 1024];
                let n = link.read(&mut request).await.unwrap();
                let line = String::from_utf8_lossy(&request[..n])
                    .lines()
                    .next()
                    .unwrap()
                    .to_string();
                let body = format!("port {port}: {line}");
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                link.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        // Nothing listens on it: held, so no other test takes it either.
        let held = link_test_kit::HeldNumber::port();
        let port = held.port_number();
        let record = dir.join("opened");
        let opener = Opener {
            sandbox: "main-otter".into(),
            dir: dir.to_path_buf(),
            launcher: Some(vec![
                "sh".into(),
                "-c".into(),
                format!("printf %s \"$0\" > {}", record.display()),
            ]),
            inbound: Some(Inbound {
                path: inbound,
                // SAFETY: geteuid has no preconditions.
                agent: unsafe { libc::geteuid() },
            }),
            hosts: vec!["*".into()],
            desktop: Vec::new(),
            asking: None,
        };
        let url = format!(
            "https://id.example.com/auth?client_id=x&redirect_uri=http%3A%2F%2F127.0.0.1%3A{port}%2Fcb"
        );

        // Refused: nothing opens, nothing listens.
        let answerer = answering(&dir, false);
        let answer = opened(&opener, &url).await;
        assert!(answerer.join().unwrap());
        assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
        assert!(answer.contains("not authorized"), "{answer}");
        assert!(!record.exists());
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());

        // Allowed: opened with the whole URL, and the port served in.
        let answerer = answering(&dir, true);
        let answer = opened(&opener, &url).await;
        assert!(answerer.join().unwrap());
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        assert_eq!(std::fs::read_to_string(&record).unwrap(), url);
        let mut browser = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        browser
            .write_all(b"GET /cb?code=secret HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut reply = String::new();
        browser.read_to_string(&mut reply).await.unwrap();
        assert!(
            reply.ends_with(&format!("port {port}: GET /cb?code=secret HTTP/1.1")),
            "{reply}"
        );
        // Its request arrived: the port is closed again.
        let mut closed = false;
        for _ in 0..50 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
                closed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(closed, "the callback port closes once its request came");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn off_its_hosts_or_with_no_desktop_nothing_asks() {
        let url = "https://id.example.com/auth";
        let (_root, dir) = scratch("hosts");
        let opener = Opener {
            sandbox: "t".into(),
            dir: dir.to_path_buf(),
            launcher: Some(vec!["false".into()]),
            inbound: None,
            hosts: vec!["api.anthropic.com".into()],
            desktop: Vec::new(),
            asking: None,
        };
        let answer = opened(&opener, url).await;
        assert!(answer.contains("isn't one of the hosts"), "{answer}");
        let under = Opener {
            hosts: vec!["*.example.com".into()],
            launcher: None,
            ..opener.clone()
        };
        let answer = opened(&under, url).await;
        assert!(!answer.contains("isn't one of the hosts"), "{answer}");
        // No desktop: refused before asking.
        assert!(answer.contains("no desktop"), "{answer}");
        let answer = opened(&under, "http://id.example.com/").await;
        assert!(answer.contains("only https://"), "{answer}");
        assert!(crate::bridges::asks::waiting(&dir).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An agent's page is on its route's hosts and the hosts it's
    /// granted at the call: a browser grant taken back since its proxy
    /// started opens nothing.
    #[tokio::test]
    async fn an_agents_page_is_on_what_its_granted_now() {
        let url = "https://id.example.com/auth";
        let (_root, dir) = scratch("granted");
        let opener = Opener {
            sandbox: "t".into(),
            dir: dir.to_path_buf(),
            launcher: None,
            inbound: None,
            hosts: vec!["*".into()],
            desktop: Vec::new(),
            asking: None,
        };
        let granted = |browser: Option<&[&str]>| {
            Authority::Agent(link_sandbox_client::grants::Current {
                tools: Vec::new(),
                browser: browser.map(|b| b.iter().map(|h| h.to_string()).collect()),
            })
        };
        let answer = opened_as(&opener, url, granted(None)).await;
        assert!(answer.contains("isn't one of the hosts"), "{answer}");
        let answer = opened_as(&opener, url, granted(Some(&["github.com"]))).await;
        assert!(answer.contains("isn't one of the hosts"), "{answer}");
        // Granted now: past the hosts, then refused for no desktop.
        let answer = opened_as(&opener, url, granted(Some(&["*.example.com"]))).await;
        assert!(answer.contains("no desktop"), "{answer}");
        assert!(crate::bridges::asks::waiting(&dir).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn it_asks_with_the_whole_url() {
        let url = format!("https://id.example.com/a?pad={}", "x".repeat(1500));
        let page = check_url(&url).unwrap();
        let text = question("main-otter", &url, &page, false).text();
        assert!(text.contains(&url), "the whole URL, never cut short");
        assert!(text.starts_with("main-otter wants to open a page on id.example.com"));
    }
}
