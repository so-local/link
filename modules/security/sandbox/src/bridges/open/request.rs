//! In the sandbox: a page asked for of its proxy, as `$BROWSER`.

use std::time::Duration;

use anyhow::ensure;
use link_format::WithinExt;

use link_sandbox_client::routes::OPEN_VARIABLE as VARIABLE;

/// Where `$LNK_SANDBOX_OPEN` (`verb`) asks: a proxy on this machine's
/// loopback, and the verb's path, behind its secret.
fn verb_at(verb: &str) -> anyhow::Result<(std::net::SocketAddr, String)> {
    let parsed = verb.strip_prefix("http://").and_then(|rest| {
        let (addr, path) = rest.split_at(rest.find('/')?);
        let addr: std::net::SocketAddr = addr.parse().ok()?;
        let secret = path
            .strip_prefix(link_sandbox_route::SECRET_PREFIX)?
            .strip_suffix(link_sandbox_client::routes::OPEN.strip_prefix("/_link")?)?;
        (addr.ip().is_loopback() && link_sandbox_route::is_url_secret(secret))
            .then(|| (addr, path.to_string()))
    });
    parsed.within(format!("{VARIABLE} {verb:?} isn't its sandbox's proxy"))
}

/// Whether `text` is a page a browser is handed: an absolute HTTP or
/// HTTPS address.
pub fn is_page(text: &str) -> bool {
    text.starts_with("http://") || text.starts_with("https://")
}

/// In the sandbox: asks its proxy (`$LNK_SANDBOX_OPEN`) to open `url` in
/// your browser, and waits for your answer. What `$BROWSER` runs.
pub fn request(url: &str) -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let verb = std::env::var(VARIABLE).within(format!(
        "no {VARIABLE}: not in a proxied sandbox allowed to open pages"
    ))?;
    let (addr, path) = verb_at(&verb)?;
    let mut stream = std::net::TcpStream::connect(addr)
        .within(format!("connecting to the sandbox's proxy at {addr}"))?;
    // Your answer can take its while, as long as its agent's wait or the
    // sandbox's own (which this side can't know), and opening a few
    // seconds more.
    stream.set_read_timeout(Some(crate::bridges::asks::MOST + Duration::from_secs(60)))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: text/plain\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{url}",
        url.len()
    );
    stream.write_all(request.as_bytes())?;
    let mut reply = Vec::new();
    stream
        .take(64 * 1024)
        .read_to_end(&mut reply)
        .within("reading the proxy's answer")?;
    let reply = String::from_utf8_lossy(&reply);
    let (head, body) = reply.split_once("\r\n\r\n").unwrap_or((&reply, ""));
    let status = head.lines().next().unwrap_or_default();
    let body = link_format::clean(body.trim());
    ensure!(
        status.split(' ').nth(1) == Some("200"),
        "{}",
        match body.is_empty() {
            true => format!(
                "the sandbox's proxy didn't open it: {}",
                link_format::clean(status)
            ),
            false => body
                .strip_prefix("Link's sandbox: ")
                .unwrap_or(&body)
                .to_string(),
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The secret of the proxy's URLs in these tests.
    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn the_browser_asks_only_its_own_proxy() {
        let open = format!(
            "http://127.0.0.1:40781{}{SECRET}/web/open",
            link_sandbox_route::SECRET_PREFIX
        );
        let (addr, path) = verb_at(&open).unwrap();
        assert_eq!(addr, "127.0.0.1:40781".parse().unwrap());
        assert_eq!(path, format!("/_link/url/{SECRET}/web/open"));
        for bad in [
            "http://127.0.0.1:40781/_link/web/open",
            "http://10.0.0.1:40781/_link/url/0123456789abcdef0123456789abcdef/web/open",
            "http://127.0.0.1:40781/_link/url/short/web/open",
            "http://127.0.0.1:40781/_link/url/0123456789abcdef0123456789abcdef/mcp/gh",
            "https://127.0.0.1:40781/_link/url/0123456789abcdef0123456789abcdef/web/open",
            "",
        ] {
            assert!(verb_at(bad).is_err(), "{bad}");
        }
    }
}
