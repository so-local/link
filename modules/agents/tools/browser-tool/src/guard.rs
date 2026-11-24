//! What the browser may open and where it may keep its profile: an
//! address checked before it's opened, so a refusal says why in the
//! tool's words (the sandbox's proxy holds to the same rule as each page
//! is reached), and a profile that's never one of your own browsers'.

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

/// A host of `--private-host`: of your network, or a port here.
#[derive(Clone, Debug, PartialEq)]
pub enum Private {
    Lan(String),
    Port(u16),
}

impl Private {
    /// `text` as a private host: `localhost:<port>`, `127.0.0.1:<port>`
    /// or `[::1]:<port>` a port here, any other host one of your network.
    pub fn parse(text: &str) -> Result<Private> {
        let text = text.trim();
        if let Some((host, port)) = text.rsplit_once(':') {
            let host = host.trim_start_matches('[').trim_end_matches(']');
            let here = host == "localhost"
                || host
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified());
            if here {
                let port = port
                    .parse::<u16>()
                    .ok()
                    .filter(|p| *p > 0)
                    .with_context(|| format!("--private-host {text}: a port of this machine"))?;
                return Ok(Private::Port(port));
            }
        }
        ensure!(
            !text.is_empty() && !text.contains(char::is_whitespace),
            "--private-host {text:?}: a host"
        );
        // A port given beside a host of your network: the sandbox opens
        // the host whole.
        let host = match text.rsplit_once(':') {
            Some((host, port)) if port.parse::<u16>().is_ok() && !host.contains(':') => host,
            _ => text,
        };
        ensure!(
            host != "localhost" && !host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()),
            "--private-host {text}: this machine is named with its port, localhost:<port>"
        );
        Ok(Private::Lan(host.to_string()))
    }

    /// The sandbox's flags that let it be reached.
    pub(crate) fn flags(&self) -> [String; 2] {
        match self {
            Private::Lan(host) => ["--lan.hosts".into(), host.clone()],
            Private::Port(port) => ["--localhost.ports".into(), port.to_string()],
        }
    }

    /// Whether it names `url`, whose address is `ip` when it's one.
    fn names(&self, url: &url::Url, ip: Option<IpAddr>) -> bool {
        match self {
            Private::Port(port) => {
                (here_by_name(url) || ip.is_some_and(|ip| ip.is_loopback()))
                    && url.port_or_known_default() == Some(*port)
            }
            Private::Lan(host) => match (host.split_once('/'), ip) {
                // A range holds only the addresses in it.
                (Some((base, bits)), Some(ip)) => in_range(ip, base, bits),
                (Some(_), None) => false,
                (None, _) => url
                    .host_str()
                    .is_some_and(|h| h.trim_matches(['[', ']']).eq_ignore_ascii_case(host)),
            },
        }
    }
}

/// Whether `url`'s host is a name for this machine: `localhost`, its
/// fully qualified `localhost.`, or any name under it (`a.localhost`),
/// which a browser resolves here without asking DNS (RFC 6761).
fn here_by_name(url: &url::Url) -> bool {
    url.host_str().is_some_and(|host| {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        host == "localhost" || host.ends_with(".localhost")
    })
}

/// Whether `ip` is in the range `base`/`bits` (`192.168.1.0/24`).
fn in_range(ip: IpAddr, base: &str, bits: &str) -> bool {
    let (Ok(base), Ok(bits)) = (base.parse::<IpAddr>(), bits.parse::<u32>()) else {
        return false;
    };
    let ip = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    };
    match (ip, base) {
        (IpAddr::V4(ip), IpAddr::V4(base)) if bits <= 32 => {
            let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
            u32::from(ip) & mask == u32::from(base) & mask
        }
        (IpAddr::V6(ip), IpAddr::V6(base)) if bits <= 128 => {
            let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
            u128::from(ip) & mask == u128::from(base) & mask
        }
        _ => false,
    }
}

/// `url`, if it may be opened: the web's (`http:`, `https:`), and not an
/// address on this machine or its networks that `private` doesn't name.
/// A name is checked by the sandbox's proxy as it's reached.
pub(crate) fn allowed(url: &str, private: &[Private]) -> Result<url::Url> {
    let parsed = url::Url::parse(url.trim()).with_context(|| format!("{url:?} isn't a URL"))?;
    ensure!(
        matches!(parsed.scheme(), "http" | "https"),
        "only http:// and https:// pages open, not {}:",
        parsed.scheme()
    );
    let ip = match parsed.host() {
        Some(url::Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
        _ => None,
    };
    let here = here_by_name(&parsed) || ip.is_some_and(link_sandbox_contract::net::forbidden);
    if here && !private.iter().any(|p| p.names(&parsed, ip)) {
        bail!(
            "{} is on this machine or its network, which the browser opens only when \
             you name it (lnk-browser-tool serve --private-host)",
            parsed.host_str().unwrap_or("")
        );
    }
    Ok(parsed)
}

/// Refuses `profile` if it's one of your own browsers' profiles, or in
/// one, or can't be told apart from them: with no home to find them in.
pub(crate) fn own_profile(profile: &Path) -> Result<()> {
    ensure!(
        profile.is_absolute(),
        "the profile {} isn't a full path",
        profile.display()
    );
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .context(
            "HOME isn't set to a full path, so the browser can't tell its profile from your \
             own browsers': set HOME",
        )?;
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let profile = canonical(profile);
    for own in &crate::settings::get().own_profiles {
        let own = canonical(&home.join(own));
        if profile.starts_with(&own) {
            bail!(
                "{} is your own browser's profile: this tool keeps one of its own",
                profile.display()
            );
        }
    }
    Ok(())
}
