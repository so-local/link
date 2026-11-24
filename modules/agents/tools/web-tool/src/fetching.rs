//! Fetching a page: which addresses it may reach, and the answer read by
//! its kind.

use super::*;

impl Web {
    /// Whether `host` is one you named, its addresses fetched wherever
    /// they are.
    fn named_private(&self, host: &str) -> bool {
        let host = host.trim_start_matches('[').trim_end_matches(']');
        self.private_hosts
            .iter()
            .any(|h| h.eq_ignore_ascii_case(host))
    }

    /// The addresses `url` is fetched from: each the name resolves to,
    /// or the address it is, all of them allowed, else an error naming
    /// why. A name that resolves to one forbidden address among allowed
    /// ones is refused whole: which one a connection takes isn't ours
    /// to choose. A host the agent's spec doesn't name, where it names
    /// any (`hosts`), is refused before it's resolved.
    fn addresses(
        &self,
        url: &Url,
        left: Duration,
        hosts: Option<&[String]>,
    ) -> anyhow::Result<Vec<SocketAddr>> {
        ensure!(
            matches!(url.scheme(), "http" | "https"),
            "only http:// and https:// pages are fetched, not {}:",
            link_format::clean(url.scheme())
        );
        ensure!(
            url.username().is_empty() && url.password().is_none(),
            "a URL with a login in it isn't fetched"
        );
        let host = url.host_str().context("the URL names no host")?;
        listed(url, hosts)?;
        let port = url
            .port_or_known_default()
            .context("the URL names no port")?;
        let bare = host.trim_start_matches('[').trim_end_matches(']');
        let addresses: Vec<SocketAddr> = match bare.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, port)],
            Err(_) => resolved(bare, port, left)?,
        };
        ensure!(
            !addresses.is_empty(),
            "{} resolves to no address",
            link_format::clean(bare)
        );
        if self.named_private(host) {
            return Ok(addresses);
        }
        for address in &addresses {
            let ip = address.ip();
            if link_sandbox_contract::net::forbidden(ip)
                || link_sandbox_contract::net::this_machine(ip)
            {
                bail!(
                    "{} is on this machine or its networks ({ip}), which this tool never reaches",
                    link_format::clean(bare)
                );
            }
        }
        Ok(addresses)
    }

    /// [`Web::fetch`], or, for a part after the first (`again`), the
    /// page kept from a fetch of `url` in the last `page-kept-seconds`: one page
    /// read through, not fetched again for each part. A first part is
    /// always fetched, so the page is as it is now.
    /// A page kept is given only where `hosts` names both the address
    /// asked for and where it ended.
    pub fn page(&self, url: &str, again: bool, hosts: Option<&[String]>) -> anyhow::Result<Page> {
        let mut kept = self.pages.0.lock().unwrap_or_else(|e| e.into_inner());
        kept.retain(|(_, at, _)| at.elapsed() < crate::limits().page_kept());
        if again && let Some((_, _, page)) = kept.iter().find(|(asked, _, _)| asked == url) {
            for address in [url, page.url.as_str()] {
                if let Ok(address) = Url::parse(address) {
                    listed(&address, hosts)?;
                }
            }
            return Ok(page.clone());
        }
        drop(kept);
        let page = self.fetch(url, hosts)?;
        let mut kept = self.pages.0.lock().unwrap_or_else(|e| e.into_inner());
        kept.retain(|(asked, _, _)| asked != url);
        kept.push((url.to_string(), std::time::Instant::now(), page.clone()));
        let over = kept.len().saturating_sub(crate::limits().pages);
        kept.drain(..over);
        Ok(page)
    }

    /// Fetches `url`: its text, markdown for a page of HTML, read up to
    /// `page-max-bytes`. Each redirect is checked as the page was, on
    /// `hosts` where the agent's spec names any (`[tool.web] hosts`).
    pub fn fetch(&self, url: &str, hosts: Option<&[String]>) -> anyhow::Result<Page> {
        let mut url = Url::parse(url.trim())
            .with_context(|| format!("{:?} isn't a URL", link_format::clean(url)))?;
        url.set_fragment(None);
        // One deadline for the page, its redirects and its body.
        let limits = crate::limits();
        let deadline = std::time::Instant::now() + limits.timeout();
        for _ in 0..=limits.redirects {
            let left = deadline
                .checked_duration_since(std::time::Instant::now())
                .filter(|left| !left.is_zero())
                .with_context(|| format!("no page in {}s", limits.timeout().as_secs()))?;
            let addresses = self.addresses(&url, left, hosts)?;
            let host = url.host_str().unwrap_or_default().to_string();
            // Connect to the addresses checked, never resolve again.
            let response = self
                .clients
                .pages(&host, &addresses)?
                .get(url.clone())
                .timeout(left)
                .header(
                    reqwest::header::ACCEPT,
                    "text/html,application/xhtml+xml,text/markdown;q=0.9,text/plain;q=0.9,*/*;q=0.5",
                )
                .send()
                .map_err(|err| anyhow::anyhow!("{}", without_url(&err)))
                .with_context(|| format!("fetching {}", link_format::clean(url.as_str())))?;
            let status = response.status();
            if status.is_redirection() {
                let to = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|l| l.to_str().ok())
                    .context("a redirect without where to")?;
                let next = url
                    .join(to)
                    .with_context(|| format!("a redirect to {:?}", link_format::clean(to)))?;
                // Never from https to http, where what's sent and read
                // can be seen and changed on the way.
                ensure!(
                    !(url.scheme() == "https" && next.scheme() == "http"),
                    "{} redirects to {}, without https: not followed",
                    link_format::clean(url.as_str()),
                    link_format::clean(next.as_str())
                );
                url = next;
                url.set_fragment(None);
                continue;
            }
            ensure!(
                status.is_success(),
                "{} answered {status}",
                link_format::clean(url.as_str())
            );
            return read_page(url, response, deadline);
        }
        bail!("more than {} redirects", limits.redirects)
    }
}

/// An error unless `url`'s host is one of `hosts`, the agent's spec's
/// `[tool.web] hosts`, as `[internet] hosts` names them: a name, `*.` and
/// a name for every name under it, or an address. None, where the spec
/// names none, allows any.
fn listed(url: &Url, hosts: Option<&[String]>) -> anyhow::Result<()> {
    let Some(hosts) = hosts else {
        return Ok(());
    };
    let host = url.host_str().unwrap_or_default();
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    ensure!(
        link_sandbox_contract::net::host_allowed(bare, hosts),
        "{} isn't a host the agent's spec lets this tool fetch ([tool.web] hosts)",
        link_format::clean(bare)
    );
    Ok(())
}

/// The addresses name `host` resolves to at `port`, looked up within
/// `left`: the system's lookup has no deadline of its own, so it runs on
/// a thread of its own, left to end alone when it takes longer.
fn resolved(host: &str, port: u16, left: Duration) -> anyhow::Result<Vec<SocketAddr>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let name = host.to_string();
    std::thread::spawn(move || {
        let _ = tx.send((name.as_str(), port).to_socket_addrs().map(Vec::from_iter));
    });
    match rx.recv_timeout(left) {
        Ok(found) => found
            .with_context(|| format!("{} isn't a name that resolves", link_format::clean(host))),
        Err(_) => bail!(
            "{} didn't resolve in {}s",
            link_format::clean(host),
            crate::limits().timeout().as_secs()
        ),
    }
}

/// An error's words without its URL, its causes kept: a timeout, a
/// refused connection, and a certificate each say so, where reqwest's own
/// words are "error sending request" for all of them. A search API's
/// URL carries the query, never the key, but a page's may carry what the
/// agent put in it, so neither is said.
pub(super) fn without_url(err: &reqwest::Error) -> String {
    let mut words = err.to_string();
    if let Some(at) = words.find(" for url (") {
        words.truncate(at);
    }
    if err.is_timeout() && !words.contains("timed out") {
        words.push_str(": timed out");
    }
    let mut cause = std::error::Error::source(err);
    while let Some(err) = cause {
        let said = err.to_string();
        if !said.is_empty() && !words.contains(&said) {
            words.push_str(": ");
            words.push_str(&said);
        }
        cause = err.source();
    }
    words
}

/// What kind of text a response is, by its `Content-Type`: HTML, other
/// text, or a kind it doesn't read.
#[derive(Debug, PartialEq)]
enum Kind {
    Html,
    Text,
    Other(String),
}

/// The types that say nothing of what a body is, so it's read by how
/// it begins, as a body of no type is: a text file a server doesn't
/// know is often sent as one of these.
const UNSAID: [&str; 2] = ["application/octet-stream", "binary/octet-stream"];

fn kind(content_type: Option<&str>, start: &[u8]) -> Kind {
    let Some(content_type) = content_type else {
        return sniffed(start, "a file of no type");
    };
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "text/html" | "application/xhtml+xml" => Kind::Html,
        _ if UNSAID.contains(&mime.as_str()) => sniffed(start, &mime),
        _ if mime.starts_with("text/")
            || mime.ends_with("+json")
            || mime.ends_with("+xml")
            || matches!(
                mime.as_str(),
                "application/json" | "application/xml" | "application/javascript"
            ) =>
        {
            Kind::Text
        }
        _ => Kind::Other(link_format::clean(&mime)),
    }
}

/// A body whose type says nothing, by how it begins: HTML if it looks
/// like it, text if it has no NUL and every byte reads (a character cut
/// at the end of what's read still counts) in the set its byte order
/// mark names (UTF-8's or UTF-16's), or in UTF-8 when it has none, else
/// `what`, a kind it doesn't read.
fn sniffed(start: &[u8], what: &str) -> Kind {
    let start = &start[..start.len().min(crate::limits::limits().sniff_bytes)];
    let marked = encoding_rs::Encoding::for_bom(start).map(|(encoding, len)| {
        let mut rest = &start[len..];
        // A unit cut where the read stopped is no character yet.
        if encoding != encoding_rs::UTF_8 {
            rest = &rest[..rest.len() & !1];
        }
        encoding.decode_without_bom_handling(rest).0
    });
    // Read in its set, text has no NUL, and no byte its set can't read
    // but a character cut at the end.
    let marked_text = marked.as_deref().is_some_and(|text| {
        let whole = text.strip_suffix('\u{FFFD}').unwrap_or(text);
        !whole.contains(['\0', '\u{FFFD}'])
    });
    let head = match &marked {
        Some(text) => text.to_ascii_lowercase(),
        None => String::from_utf8_lossy(start).to_ascii_lowercase(),
    };
    let head = head.trim_start();
    let text = marked_text
        || (marked.is_none()
            && !start.contains(&0)
            && std::str::from_utf8(start).map_or_else(|e| e.error_len().is_none(), |_| true));
    match () {
        _ if head.starts_with("<!doctype html") || head.starts_with("<html") => Kind::Html,
        _ if text => Kind::Text,
        _ => Kind::Other(what.into()),
    }
}

/// A response's page: up to `page-max-bytes` of it, as markdown or
/// text, read by `deadline`.
fn read_page(
    url: Url,
    response: reqwest::blocking::Response,
    deadline: std::time::Instant,
) -> anyhow::Result<Page> {
    let header = |name| {
        response
            .headers()
            .get(name)
            .and_then(|t| t.to_str().ok())
            .map(str::to_string)
    };
    let content_type = header(reqwest::header::CONTENT_TYPE);
    let encoding = header(reqwest::header::CONTENT_ENCODING);
    let not_text = |what: String| {
        anyhow::anyhow!(
            "{} is {what}, not a page of text, which this tool doesn't read",
            link_format::clean(url.as_str())
        )
    };
    // Its type said: one it doesn't read is refused before its body.
    if content_type.is_some()
        && let Kind::Other(what) = kind(content_type.as_deref(), &[])
    {
        return Err(not_text(what));
    }
    let page_max = crate::limits().page_max_bytes;
    let mut body = Vec::new();
    let mut response = response.take(page_max + 1);
    let mut chunk = vec![0; crate::limits::limits().read_chunk_bytes];
    // A page sent a little at a time still ends at the deadline.
    loop {
        ensure!(
            std::time::Instant::now() < deadline,
            "the page didn't come within {}s",
            crate::limits().timeout().as_secs()
        );
        match response.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err).context("reading the page"),
        }
    }
    let sent_cut = body.len() as u64 > page_max;
    body.truncate(page_max as usize);
    let mut body = crate::inflating::inflated(encoding.as_deref(), body, page_max + 1)?;
    let cut = sent_cut || body.len() as u64 > page_max;
    body.truncate(page_max as usize);
    let mut text = decoded(content_type.as_deref(), &body);
    // A set of one byte a character can grow to three as UTF-8: the page
    // read stays within the most it reads.
    let cut = cut || text.len() as u64 > page_max;
    text.truncate(text.floor_char_boundary(page_max as usize));
    let (title, text) = match kind(content_type.as_deref(), &body) {
        Kind::Html => {
            // Its tree takes many times the page: a few at once.
            let _converting = CONVERTING.take();
            (title(&text), markdown(&text, &url))
        }
        Kind::Text => (None, text),
        Kind::Other(what) => return Err(not_text(what)),
    };
    Ok(Page {
        url: url.to_string(),
        title,
        text,
        cut,
    })
}

/// `body`'s text, in the character set it's written in: its byte order
/// mark's, else the one its `Content-Type` names, else an HTML page's
/// `<meta>` one, else UTF-8. A byte its set can't read becomes U+FFFD.
fn decoded(content_type: Option<&str>, body: &[u8]) -> String {
    let named = |label: &str| {
        encoding_rs::Encoding::for_label(label.trim().trim_matches(['"', '\'']).as_bytes())
    };
    // A parameter's name is any case, with space about its `=`.
    let declared = content_type
        .and_then(|t| {
            t.split(';').skip(1).find_map(|p| {
                let (name, value) = p.split_once('=')?;
                name.trim().eq_ignore_ascii_case("charset").then_some(value)
            })
        })
        .and_then(named);
    // A `<meta>` can't name UTF-16, whose page it is read in, so one that
    // says so is UTF-8, and `x-user-defined` windows-1252, as a browser
    // reads them.
    let meta = || {
        meta_charset(body)
            .and_then(|label| named(&label))
            .map(|e| match e {
                e if e == encoding_rs::UTF_16LE || e == encoding_rs::UTF_16BE => encoding_rs::UTF_8,
                e if e == encoding_rs::X_USER_DEFINED => encoding_rs::WINDOWS_1252,
                e => e,
            })
    };
    let encoding = encoding_rs::Encoding::for_bom(body)
        .map(|(encoding, _)| encoding)
        .or(declared)
        .or_else(meta)
        .unwrap_or(encoding_rs::UTF_8);
    // The mark, where there's one, is dropped, as a browser does.
    encoding.decode(body).0.into_owned()
}

/// The character set an HTML page's `<meta>` names in its start
/// (`meta-scan-bytes`): `<meta charset="...">`, or
/// `content="...; charset=..."`.
fn meta_charset(body: &[u8]) -> Option<String> {
    let scan = crate::limits::limits().meta_scan_bytes;
    let head = String::from_utf8_lossy(&body[..body.len().min(scan)]).to_ascii_lowercase();
    let mut rest = head.as_str();
    while let Some(at) = rest.find("<meta") {
        rest = &rest[at + 5..];
        let tag = &rest[..rest.find('>').unwrap_or(rest.len())];
        if let Some(at) = tag.find("charset=") {
            let value = tag[at + 8..].trim_start_matches(['"', '\'']);
            let end = value
                .find(|c: char| !(c.is_ascii_alphanumeric() || "-_:.".contains(c)))
                .unwrap_or(value.len());
            if end > 0 {
                return Some(value[..end].to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session's fetches share one client, its connections kept, and a
    /// host is connected to at the addresses it was checked at, never
    /// resolved again.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn fetches_share_a_client_pinned_to_the_addresses_checked() {
        // This fixture exercises a direct connection to a deliberately
        // unresolvable name. An inherited proxy would resolve it itself.
        // Isolate the test in a child rather than mutate the process-wide
        // environment while other tests use their HTTP clients.
        const PROXIES: &[&str] = &[
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
        ];
        if PROXIES.iter().any(|name| std::env::var_os(name).is_some()) {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.args([
                "--exact",
                "fetching::tests::fetches_share_a_client_pinned_to_the_addresses_checked",
                "--nocapture",
            ]);
            for name in PROXIES {
                child.env_remove(name);
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("1 passed;"),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        link_fetch::install_crypto_provider();
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let connections = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = connections.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    loop {
                        let mut line = String::new();
                        loop {
                            line.clear();
                            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                                return;
                            }
                            if line == "\r\n" {
                                break;
                            }
                        }
                        let body = "kept";
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        );
                    }
                });
            }
        });
        let web = Web {
            private_hosts: vec!["127.0.0.1".into()],
            ..Web::default()
        };
        let url = format!("http://{address}/");
        for _ in 0..3 {
            assert_eq!(web.fetch(&url, None).unwrap().text, "kept");
        }
        assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 1);
        // A name is answered with its pinned addresses: one no resolver
        // knows reaches the server it was pinned to.
        let client = web.clients.pages("pinned.invalid", &[address]).unwrap();
        let body = client
            .get(format!("http://pinned.invalid:{}/", address.port()))
            .send()
            .unwrap()
            .text()
            .unwrap();
        assert_eq!(body, "kept");
    }

    #[test]
    fn this_machine_and_its_networks_are_never_fetched() {
        let web = Web::default();
        for url in [
            "http://127.0.0.1/",
            "http://localhost:8080/admin",
            "http://10.0.0.1/",
            "http://192.168.1.1/",
            "http://172.16.0.5/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[fd00::1]/",
            "http://100.100.100.100/",
            "http://0.0.0.0/",
            "http://2130706433/",
            "http://0x7f.1/",
            "file:///etc/passwd",
            "ftp://example.com/",
            "http://user:secret@example.com/",
        ] {
            let url = Url::parse(url).unwrap();
            assert!(
                web.addresses(&url, crate::limits().timeout(), None)
                    .is_err(),
                "{url}"
            );
        }
        // A host you named is fetched wherever it is.
        let web = Web {
            private_hosts: vec!["127.0.0.1".into(), "localhost".into()],
            ..Web::default()
        };
        for url in ["http://127.0.0.1:9/", "http://localhost:9/"] {
            assert!(
                web.addresses(&Url::parse(url).unwrap(), crate::limits().timeout(), None)
                    .is_ok(),
                "{url}"
            );
        }
        // Naming one doesn't name another.
        assert!(
            web.addresses(
                &Url::parse("http://10.0.0.1/").unwrap(),
                crate::limits().timeout(),
                None
            )
            .is_err()
        );
    }

    /// Where the agent's spec names hosts (`[tool.web] hosts`), a fetch
    /// reaches those alone, a `*.` one holding the names under it, and a
    /// private host you named is held to them too; naming none allows
    /// any.
    #[test]
    fn a_fetch_reaches_only_the_specs_hosts() {
        let web = Web {
            private_hosts: vec!["127.0.0.1".into()],
            ..Web::default()
        };
        let hosts = ["docs.example.org".to_string(), "*.example.com".to_string()];
        let reaches = |url: &str, hosts: Option<&[String]>| {
            web.addresses(&Url::parse(url).unwrap(), crate::limits().timeout(), hosts)
        };
        for url in [
            "http://www.example.net/",
            "http://example.com/",
            "http://127.0.0.1:9/",
        ] {
            let refused = format!("{:#}", reaches(url, Some(&hosts)).unwrap_err());
            assert!(refused.contains("[tool.web] hosts"), "{url}: {refused}");
        }
        assert!(reaches("http://127.0.0.1:9/", None).is_ok());
        let named = ["127.0.0.1".to_string()];
        assert!(reaches("http://127.0.0.1:9/", Some(&named)).is_ok());
        // A kept page is held to them as a fetched one is.
        let url = "http://127.0.0.1:9/kept";
        web.pages.0.lock().unwrap().push((
            url.into(),
            std::time::Instant::now(),
            Page {
                url: url.into(),
                title: None,
                text: "kept".into(),
                cut: false,
            },
        ));
        assert_eq!(web.page(url, true, Some(&named)).unwrap().text, "kept");
        assert!(web.page(url, true, Some(&hosts)).is_err());
        assert!(listed(&Url::parse("https://a.example.com/").unwrap(), Some(&hosts)).is_ok());
        assert!(
            listed(
                &Url::parse("https://docs.example.org/x").unwrap(),
                Some(&hosts)
            )
            .is_ok()
        );
    }

    /// A page is read in the character set it's written in: its byte
    /// order mark's, its `Content-Type`'s, then its `<meta>`'s, else UTF-8.
    #[test]
    fn a_page_is_read_in_its_character_set() {
        let (shift_jis, _, _) = encoding_rs::SHIFT_JIS.encode("日本語のページ");
        assert_eq!(
            decoded(Some("text/html; charset=Shift_JIS"), &shift_jis),
            "日本語のページ"
        );
        let (windows, _, _) = encoding_rs::WINDOWS_1252.encode("café – naïve");
        assert_eq!(
            decoded(Some("text/plain; charset=\"windows-1252\""), &windows),
            "café – naïve"
        );
        let mut page = b"<html><head><meta charset='windows-1252'></head><body>".to_vec();
        page.extend_from_slice(&windows);
        assert!(decoded(Some("text/html"), &page).ends_with("café – naïve"));
        let mut equiv =
            b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=shift_jis\">".to_vec();
        equiv.extend_from_slice(&shift_jis);
        assert!(decoded(None, &equiv).ends_with("日本語のページ"));
        // A mark wins over what's declared, and is dropped.
        let marked = [b"\xEF\xBB\xBF".as_slice(), "é".as_bytes()].concat();
        assert_eq!(
            decoded(Some("text/plain; charset=windows-1252"), &marked),
            "é"
        );
        // Nothing said: UTF-8, a byte it can't read replaced.
        assert_eq!(decoded(None, b"ok \xff"), "ok \u{fffd}");
        // A parameter's name is any case, with space about its `=`.
        assert_eq!(
            decoded(Some("text/plain; Charset = windows-1252"), &windows),
            "café – naïve"
        );
        // A `<meta>` naming UTF-16 is read as UTF-8, x-user-defined as
        // windows-1252.
        let utf16 = "<meta charset=\"utf-16\"><p>café</p>";
        assert_eq!(decoded(None, utf16.as_bytes()), utf16);
        let mut user = b"<meta charset=x-user-defined>".to_vec();
        user.extend_from_slice(&windows);
        assert!(decoded(None, &user).ends_with("café – naïve"));
    }

    #[test]
    fn a_response_is_read_by_its_kind() {
        assert_eq!(kind(Some("text/html; charset=utf-8"), b""), Kind::Html);
        assert_eq!(kind(Some("application/json"), b"{}"), Kind::Text);
        assert_eq!(kind(Some("text/markdown"), b"#"), Kind::Text);
        assert_eq!(kind(Some("application/rss+xml"), b"<"), Kind::Text);
        assert!(matches!(
            kind(Some("application/pdf"), b"%PDF"),
            Kind::Other(_)
        ));
        assert!(matches!(kind(Some("image/png"), b""), Kind::Other(_)));
        assert_eq!(kind(None, b"<!DOCTYPE html><p>"), Kind::Html);
        assert_eq!(kind(None, b"plain words"), Kind::Text);
        // A mark followed by what its set reads as NUL is no text.
        assert!(matches!(kind(None, &[0xff, 0xfe, 0, 0]), Kind::Other(_)));
        assert!(matches!(kind(None, b"\xef\xbb\xbf\0\x01"), Kind::Other(_)));
    }

    /// A failed request says why, not only that sending failed.
    #[test]
    fn a_failed_request_keeps_its_cause() {
        link_fetch::install_crypto_provider();
        let port = link_test_kit::net::HeldNumber::port();
        let err = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://127.0.0.1:{}/x", port.port_number()))
            .send()
            .unwrap_err();
        let words = without_url(&err).to_ascii_lowercase();
        assert!(words.contains("refused"), "{words}");
        assert!(!words.contains("127.0.0.1"), "{words}");
    }

    /// A name's lookup is held to the time left.
    #[test]
    fn a_lookup_is_held_to_the_time_left() {
        let err = resolved("example.invalid", 80, Duration::ZERO).unwrap_err();
        let words = format!("{err:#}");
        assert!(
            words.contains("didn't resolve") || words.contains("isn't a name"),
            "{words}"
        );
    }

    /// A body sent as octet-stream, a type that says nothing, is read by
    /// how it begins: text and HTML are read, a binary file refused by
    /// the type it was sent as, and nothing refused before its body.
    #[test]
    fn an_octet_stream_is_read_by_how_it_begins() {
        let octets = Some("application/octet-stream");
        assert_eq!(kind(octets, b""), Kind::Text);
        assert_eq!(kind(octets, b"# Notes\n\nplain words"), Kind::Text);
        assert_eq!(kind(Some("binary/octet-stream"), b"<html><p>"), Kind::Html);
        // A character cut where the read stopped is still text.
        assert_eq!(kind(octets, &"caf\u{e9}".as_bytes()[..4]), Kind::Text);
        assert_eq!(
            kind(octets, b"\x7fELF\x02\x01\x01\0"),
            Kind::Other("application/octet-stream".into())
        );
        // A UTF-16 page, its NULs and all, is read by its byte order mark,
        // as HTML when it reads as HTML in its set, and read whole.
        let utf16 = |text: &str, bom: [u8; 2], le: bool| {
            let mut bytes = bom.to_vec();
            for unit in text.encode_utf16() {
                bytes.extend(if le {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            bytes
        };
        let notes = utf16("plain words", [0xff, 0xfe], true);
        assert_eq!(kind(octets, &notes), Kind::Text);
        assert_eq!(decoded(octets, &notes), "plain words");
        let page = utf16("<!DOCTYPE html><p>hi", [0xfe, 0xff], false);
        assert_eq!(kind(octets, &page), Kind::Html);
    }
}
