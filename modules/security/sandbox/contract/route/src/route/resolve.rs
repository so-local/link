//! Names to addresses, as the proxy finds them: kept a while, each
//! asked once at a time.

use std::net::{IpAddr, SocketAddr};

use anyhow::Context;

/// How long a name's addresses are kept, and how many names.
const RESOLVED_FOR: std::time::Duration = std::time::Duration::from_secs(30);

const MAX_RESOLVED: usize = 256;

/// Names resolved lately, and when: what [`resolve`] found, before any
/// filter, which [`dial`] still applies each time.
type Resolved = std::collections::HashMap<String, (std::time::Instant, Vec<IpAddr>)>;

static RESOLVED: std::sync::Mutex<Option<Resolved>> = std::sync::Mutex::new(None);

/// `host`'s addresses, from the system's resolver, kept [`RESOLVED_FOR`]:
/// a package manager's hundreds of connections to a few hosts ask once.
pub async fn resolve(host: &str, port: u16) -> anyhow::Result<Vec<SocketAddr>> {
    resolve_held(host, port, None).await
}

/// How long a name that has no address is kept as such: a program that
/// asks again at once, as resolvers retry, asks no one.
const FAILED_FOR: std::time::Duration = std::time::Duration::from_secs(5);

/// Whether a lookup kept since `when` still holds: `ips` none, it failed.
fn fresh(when: std::time::Instant, ips: &[IpAddr], now: std::time::Instant) -> bool {
    let kept = match ips.is_empty() {
        true => FAILED_FOR,
        false => RESOLVED_FOR,
    };
    now.duration_since(when) < kept
}

/// What's kept of `name` (lowercase), on `port`: its addresses, or that
/// it has none; none when nothing is.
fn kept(name: &str, port: u16) -> Option<anyhow::Result<Vec<SocketAddr>>> {
    let now = std::time::Instant::now();
    let resolved = RESOLVED.lock().ok()?;
    let (when, ips) = resolved.as_ref()?.get(name)?;
    if !fresh(*when, ips, now) {
        return None;
    }
    Some(match ips.is_empty() {
        true => Err(anyhow::anyhow!(
            "resolving {name}: no address (asked lately)"
        )),
        false => Ok(ips.iter().map(|ip| SocketAddr::new(*ip, port)).collect()),
    })
}

/// The names being looked up now, each with the lock its lookup holds: a
/// second asker (an AAAA beside an A, connections at once to one host)
/// waits for the first's answer, rather than asking again.
type LookingUp = std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>;

static LOOKING_UP: std::sync::Mutex<Option<LookingUp>> = std::sync::Mutex::new(None);

/// Takes `name` out of [`LOOKING_UP`] when dropped, if its lock is still
/// `lock`: once its lookup is done, or given up.
struct Looked<'a>(&'a str, std::sync::Arc<tokio::sync::Mutex<()>>);

impl Drop for Looked<'_> {
    fn drop(&mut self) {
        if let Ok(mut looking) = LOOKING_UP.lock()
            && let Some(looking) = looking.as_mut()
            && looking
                .get(self.0)
                .is_some_and(|lock| std::sync::Arc::ptr_eq(lock, &self.1))
        {
            looking.remove(self.0);
        }
    }
}

/// The same, the lookup (if the answer isn't kept) holding `permit` for
/// as long as its thread runs, even past a caller that stopped waiting:
/// the sandbox's DNS ([`crate::dns`]) bounds its threads by it. A name
/// already being looked up waits for that answer; one with no address is
/// kept as such [`FAILED_FOR`].
pub async fn resolve_held(
    host: &str,
    port: u16,
    permit: Option<tokio::sync::SemaphorePermit<'static>>,
) -> anyhow::Result<Vec<SocketAddr>> {
    let name = host.to_ascii_lowercase();
    if let Some(ips) = faked(&name) {
        return Ok(ips
            .into_iter()
            .map(|ip| SocketAddr::new(ip, port))
            .collect());
    }
    if let Some(kept) = kept(&name, port) {
        return kept;
    }
    let lock = match LOOKING_UP.lock() {
        Ok(mut looking) => looking
            .get_or_insert_default()
            .entry(name.clone())
            .or_default()
            .clone(),
        Err(_) => Default::default(),
    };
    let _looked = Looked(&name, lock.clone());
    let _turn = lock.lock().await;
    // Another's lookup, done while this one waited.
    if let Some(kept) = kept(&name, port) {
        return kept;
    }
    let found: anyhow::Result<Vec<SocketAddr>> = match permit {
        None => tokio::net::lookup_host((host, port))
            .await
            .map(|addrs| addrs.collect())
            .with_context(|| format!("resolving {host}")),
        Some(permit) => {
            let asked = host.to_string();
            tokio::task::spawn_blocking(move || {
                let _held = permit;
                std::net::ToSocketAddrs::to_socket_addrs(&(asked.as_str(), port))
                    .map(|addrs| addrs.collect::<Vec<_>>())
            })
            .await?
            .with_context(|| format!("resolving {host}"))
        }
    };
    let now = std::time::Instant::now();
    if let Ok(mut resolved) = RESOLVED.lock() {
        let resolved = resolved.get_or_insert_default();
        resolved.retain(|_, (when, ips)| fresh(*when, ips, now));
        if resolved.len() < MAX_RESOLVED {
            let ips = found.iter().flatten().map(SocketAddr::ip).collect();
            resolved.insert(name.clone(), (now, ips));
        }
    }
    found
}

/// Names given their addresses without asking anyone, for tests on a
/// machine without DNS: `name=address`, comma separated, a name once
/// for each of its addresses. Read by a debug build only.
pub const TEST_RESOLVE: &str = "LNK_TEST_RESOLVE";

/// [`TEST_RESOLVE`]'s value, in a debug build where it's set.
pub fn test_resolve() -> Option<String> {
    link_secretive::test_var(TEST_RESOLVE).and_then(|v| v.into_string().ok())
}

/// `name`'s addresses as [`TEST_RESOLVE`] gives them, if it does.
fn faked(name: &str) -> Option<Vec<IpAddr>> {
    faked_in(&test_resolve()?, name)
}

/// `name`'s addresses in `names`, [`TEST_RESOLVE`]'s value, if it has any.
pub fn faked_in(names: &str, name: &str) -> Option<Vec<IpAddr>> {
    let ips: Vec<IpAddr> = names
        .split(',')
        .filter_map(|pair| {
            let (listed, ip) = pair.split_once('=')?;
            listed.trim().eq_ignore_ascii_case(name).then_some(())?;
            ip.trim().parse().ok()
        })
        .collect();
    (!ips.is_empty()).then_some(ips)
}

/// Whether `host` has an address here, as [`resolve`] finds them: an
/// address itself, or a name this machine's resolver answers.
pub fn resolves(host: &str) -> bool {
    faked(&host.to_ascii_lowercase()).is_some()
        || std::net::ToSocketAddrs::to_socket_addrs(&(host, 0))
            .is_ok_and(|mut addrs| addrs.next().is_some())
}

/// Whether `host`'s addresses are kept, or that it has none, or they're
/// being looked up: a lookup of it asks no one more.
pub fn is_resolved(host: &str) -> bool {
    let name = host.to_ascii_lowercase();
    if faked(&name).is_some() {
        return true;
    }
    let now = std::time::Instant::now();
    let kept = RESOLVED.lock().is_ok_and(|resolved| {
        resolved
            .as_ref()
            .and_then(|resolved| resolved.get(&name))
            .is_some_and(|(when, ips)| fresh(*when, ips, now))
    });
    kept || LOOKING_UP
        .lock()
        .is_ok_and(|looking| looking.as_ref().is_some_and(|l| l.contains_key(&name)))
}

/// Puts `ip` first among `host`'s addresses: the next connection tries
/// the one that answered before the others.
pub fn prefer(host: &str, ip: IpAddr) {
    if let Ok(mut resolved) = RESOLVED.lock()
        && let Some(resolved) = resolved.as_mut()
        && let Some((_, ips)) = resolved.get_mut(&host.to_ascii_lowercase())
        && let Some(at) = ips.iter().position(|known| *known == ip)
    {
        ips[..=at].rotate_right(1);
    }
}

/// Forgets `host`'s addresses, none of which answered.
pub fn forget(host: &str) {
    if let Ok(mut resolved) = RESOLVED.lock()
        && let Some(resolved) = resolved.as_mut()
    {
        resolved.remove(&host.to_ascii_lowercase());
    }
}
