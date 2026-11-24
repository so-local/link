//! What a program in a proxied sandbox is given: its proxy's address,
//! and `sandbox connect` for a protocol of its own.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;

use anyhow::{Context, ensure};
use tokio::io::AsyncWriteExt;

use crate::http::{AnswerHead, read_head};
use link_sandbox_route::io::connect_tcp;

/// Inside the sandbox (Linux): the program's HTTP(S) proxy.
pub const PROXY_PORT: u16 = 40781;

/// The user a proxy's address names with the run's secret
/// (`http://lnk:<secret>@127.0.0.1:<port>`): any, to the proxy.
pub const PROXY_USER: &str = "lnk";

/// A generated loopback proxy address whose port is assigned at startup,
/// with the run's secret before it or not. Match the whole zero port,
/// never a longer port written with zeros.
pub fn bound_port(value: &str, port: u16) -> String {
    const FROM: &str = "127.0.0.1:0";
    let to = format!("127.0.0.1:{port}");
    let mut out = String::new();
    let mut from = 0;
    for (at, _) in value.match_indices(FROM) {
        let end = at + FROM.len();
        let before = &value[..at];
        if value.as_bytes().get(end).is_some_and(u8::is_ascii_digit)
            || !(before.ends_with("http://") || before.ends_with('@') && before.contains("http://"))
        {
            continue;
        }
        out.push_str(&value[from..at]);
        out.push_str(&to);
        from = end;
    }
    out.push_str(&value[from..]);
    out
}

/// The environment that points a program at its proxy, with the run's
/// `secret` in its address where the proxy takes none without it. Python
/// follows `HTTPS_PROXY`; Node needs `NODE_USE_ENV_PROXY` too.
pub fn proxy_env(port: u16, secret: Option<&str>) -> BTreeMap<String, String> {
    let proxy = match secret {
        Some(secret) => format!("http://{PROXY_USER}:{secret}@127.0.0.1:{port}"),
        None => format!("http://127.0.0.1:{port}"),
    };
    let local = "localhost,127.0.0.1,::1".to_string();
    BTreeMap::from([
        ("HTTP_PROXY".into(), proxy.clone()),
        ("HTTPS_PROXY".into(), proxy.clone()),
        ("http_proxy".into(), proxy.clone()),
        ("https_proxy".into(), proxy),
        ("NO_PROXY".into(), local.clone()),
        ("no_proxy".into(), local),
        ("NODE_USE_ENV_PROXY".into(), "1".into()),
    ])
}

/// `GIT_SSH_COMMAND` for a proxied sandbox: ssh through its proxy, with
/// `sandbox connect` (`exe`, this program) as its ProxyCommand, so git
/// over SSH reaches a host the proxy allows. None for a path a shell
/// would read differently.
pub fn git_ssh_command(exe: &Path) -> Option<String> {
    let exe = exe.to_str()?;
    let plain = exe
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/._-+".contains(&b));
    plain.then(|| format!("ssh -o ProxyCommand='{exe} connect %h %p'"))
}

/// `sandbox connect`: stdin and stdout joined to `host:port` through the
/// proxy in `HTTPS_PROXY` (the sandbox's own, on this machine's
/// loopback), with `CONNECT`: what ssh's ProxyCommand runs, for programs
/// that speak a protocol of their own and can be given a command to
/// connect through.
pub async fn connect(host: &str, port: u16) -> anyhow::Result<()> {
    let (addr, secret) = proxy_of()?;
    ensure!(
        !host.is_empty() && !host.contains(|c: char| c.is_whitespace() || c.is_control()),
        "{host:?} isn't a host"
    );
    let target = match host.contains(':') {
        true => format!("[{host}]:{port}"),
        false => format!("{host}:{port}"),
    };
    let authorization = match secret {
        Some(secret) => {
            use base64::Engine;
            let pair = format!("{PROXY_USER}:{secret}");
            let token = base64::engine::general_purpose::STANDARD.encode(pair);
            format!("Proxy-Authorization: Basic {token}\r\n")
        }
        None => String::new(),
    };
    let mut stream = connect_tcp(addr).await?;
    stream
        .write_all(
            format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n{authorization}\r\n")
                .as_bytes(),
        )
        .await?;
    let (head, rest) = read_head(&mut stream).await?;
    let answer = AnswerHead::parse(&head).map_err(anyhow::Error::msg)?;
    let status = answer.status;
    ensure!(
        answer.code == 200,
        "the sandbox's proxy refused {target}: {} (a port but 443 is reached only as an \
         endpoint the sandbox's policy names: its proxy's tcp, --tcp {target} for lnk \
         sandbox run)",
        link_format::clean(status)
    );
    let (mut from, mut to) = stream.into_split();
    let mut stdout = tokio::fs::File::from_std(link_format::streams::Protocol::file()?);
    stdout.write_all(&rest).await?;
    stdout.flush().await?;
    let up = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut tokio::io::stdin(), &mut to).await;
        let _ = to.shutdown().await;
    });
    tokio::io::copy(&mut from, &mut stdout).await?;
    stdout.flush().await?;
    up.abort();
    Ok(())
}

/// In a proxied sandbox: its proxy's address, from `HTTPS_PROXY`, on
/// this machine's loopback, and the run's secret, where it names one.
pub fn proxy_of() -> anyhow::Result<(SocketAddr, Option<String>)> {
    let proxy = std::env::var("HTTPS_PROXY").context("no HTTPS_PROXY: not in a proxied sandbox")?;
    parse_proxy(&proxy).context("HTTPS_PROXY isn't a proxy on this machine's loopback")
}

/// The run's secret a proxy's address on this machine's loopback names
/// (`http://lnk:<secret>@127.0.0.1:<port>`), its port bound or not yet.
pub fn secret_in(proxy: &str) -> Option<String> {
    parse_proxy(proxy)?.1
}

/// `proxy`'s address on this machine's loopback, and the secret in it.
fn parse_proxy(proxy: &str) -> Option<(SocketAddr, Option<String>)> {
    let rest = proxy.strip_prefix("http://")?.trim_end_matches('/');
    let (secret, addr) = match rest.rsplit_once('@') {
        Some((user, addr)) => (Some(user.split_once(':')?.1.to_string()), addr),
        None => (None, rest),
    };
    let addr: SocketAddr = addr.parse().ok()?;
    addr.ip().is_loopback().then_some((addr, secret))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proxys_address_names_its_secret_or_none() {
        let named = proxy_env(50123, Some("0123456789abcdef"));
        assert_eq!(
            named["HTTPS_PROXY"],
            "http://lnk:0123456789abcdef@127.0.0.1:50123"
        );
        assert_eq!(
            parse_proxy(&named["HTTPS_PROXY"]),
            Some((
                "127.0.0.1:50123".parse().unwrap(),
                Some("0123456789abcdef".into())
            ))
        );
        assert_eq!(
            parse_proxy(&proxy_env(40781, None)["HTTPS_PROXY"]),
            Some(("127.0.0.1:40781".parse().unwrap(), None))
        );
        assert_eq!(parse_proxy("http://lnk:x@10.0.0.1:80"), None);
        assert_eq!(parse_proxy("http://nopassword@127.0.0.1:80"), None);
    }

    #[test]
    fn a_port_bound_at_startup_fills_an_address_with_its_secret() {
        let both = "http://lnk:abc@127.0.0.1:0 http://127.0.0.1:0/x http://127.0.0.1:01";
        assert_eq!(
            bound_port(both, 51234),
            "http://lnk:abc@127.0.0.1:51234 http://127.0.0.1:51234/x http://127.0.0.1:01"
        );
        // Not an address: left as it is.
        assert_eq!(
            bound_port("ftp://u@127.0.0.1:0 127.0.0.1:0", 1),
            "ftp://u@127.0.0.1:0 127.0.0.1:0"
        );
    }
}
