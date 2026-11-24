//! What a sandbox's names get: an address of its own, its public ones, or a refusal.

use super::*;

/// The sandbox's own addresses for names its endpoints hold: those its
/// `/etc/hosts` gives, and one from `127.1.0.1` on for each other name
/// asked for, kept while the proxy runs.
#[derive(Default)]
pub struct Names {
    by_name: HashMap<String, Ipv4Addr>,
    by_address: HashMap<Ipv4Addr, String>,
    next: u32,
}

/// The most names given an address of their own.
pub(super) const MAX_NAMES: u32 = 65_000;

impl Names {
    /// Names with `named`'s addresses (the sandbox's `/etc/hosts`).
    pub fn with(named: &[(String, u16, Ipv4Addr)]) -> Self {
        let mut names = Names::default();
        for (host, _, addr) in named {
            if !names.by_name.contains_key(host) {
                names.by_name.insert(host.clone(), *addr);
                names.by_address.insert(*addr, host.clone());
            }
        }
        names
    }

    /// `name`'s address, given one if it has none (None: all are given).
    pub fn address(&mut self, name: &str) -> Option<Ipv4Addr> {
        if let Some(addr) = self.by_name.get(name) {
            return Some(*addr);
        }
        if self.next >= MAX_NAMES {
            return None;
        }
        self.next += 1;
        let addr = Ipv4Addr::from(u32::from(Ipv4Addr::new(127, 1, 0, 0)) + self.next);
        self.by_name.insert(name.to_string(), addr);
        self.by_address.insert(addr, name.to_string());
        Some(addr)
    }

    /// The name given `addr`, if it's one of the sandbox's own.
    pub fn name(&self, addr: Ipv4Addr) -> Option<String> {
        self.by_address.get(&addr).cloned()
    }
}

/// This proxy's names: one sandbox's, for the process.
pub static NAMES: Mutex<Option<Names>> = Mutex::new(None);

/// The name `host` stands for, when it's an address the sandbox's DNS or
/// `/etc/hosts` gave one of its endpoints' names: what a connection to it
/// asks for.
pub fn named(host: &str) -> Option<String> {
    let addr: Ipv4Addr = host.parse().ok()?;
    NAMES.lock().ok()?.as_ref()?.name(addr)
}

/// Whether `name` is one a `tcp` endpoint of `rules` holds, and its
/// hosts allow.
pub(super) fn endpoint_name(name: &str, rules: &Filter) -> bool {
    hosts_allow(name, rules)
        && rules
            .tcp
            .iter()
            .filter_map(|e| link_sandbox_contract::net::check_endpoint(e).ok())
            .any(|(host, _)| link_sandbox_contract::net::host_allowed(name, &[host]))
}

/// Whether `rules` reach `name` on the internet: its hosts allow it.
pub(super) fn hosts_allow(name: &str, rules: &Filter) -> bool {
    rules.internet
        && rules
            .hosts
            .is_none_or(|hosts| link_sandbox_contract::net::host_allowed(name, hosts))
}

/// `name` without the service labels before it: `_mongodb._tcp.x.com`
/// is `x.com`'s.
pub(super) fn service_of(name: &str) -> &str {
    let mut rest = name;
    while let Some((label, after)) = rest.split_once('.') {
        if !label.starts_with('_') {
            break;
        }
        rest = after;
    }
    rest
}

/// Suffixes only a local network answers (RFC 6762's `local`, RFC
/// 8375's `home.arpa`, and those in common use), reverse lookups, and
/// those no public name has (RFC 2606's `test`, `example` and `invalid`,
/// RFC 9476's `alt`, RFC 7686's `onion`).
pub(super) const LOCAL_SUFFIXES: &[&str] = &[
    "local",
    "localhost",
    "localdomain",
    "internal",
    "intranet",
    "private",
    "corp",
    "home",
    "lan",
    "arpa",
    "test",
    "example",
    "invalid",
    "alt",
    "onion",
];

/// Whether `name` may be a name on the internet: more than one label,
/// under none of [`LOCAL_SUFFIXES`].
pub(crate) fn public_name(name: &str) -> bool {
    let last = name.rsplit('.').next().unwrap_or(name);
    name.contains('.') && !LOCAL_SUFFIXES.contains(&last)
}

/// How a query is answered: what's asked of the machine's resolver, or
/// what's answered at once.
#[derive(Debug, PartialEq)]
pub enum Plan {
    /// Refused, before anything is asked.
    Refuse(&'static str),
    /// The sandbox's own address for this endpoint's name (A), or no
    /// address (AAAA).
    Own,
    /// The name's public addresses of this type (A or AAAA).
    Public,
    /// The machine's resolver's records of this type (SRV or TXT).
    Ask,
    /// Allowed, but no records of this type are given: an empty answer.
    Empty,
}

/// How `rules` answer a query for `name` of `qtype`. With an HTTP
/// upstream, which resolves every name itself, this machine's resolver is
/// asked none a sandbox chose: an allowed name has no records here, and
/// only the endpoints' own names are answered.
pub fn plan(name: &str, qtype: u16, rules: &Filter) -> Plan {
    match (asked(name, qtype, rules), rules.upstream) {
        (Plan::Public | Plan::Ask, link_sandbox_contract::net::Upstream::Http(_)) => Plan::Empty,
        (plan, _) => plan,
    }
}

/// [`plan`], with the machine's resolver to ask.
fn asked(name: &str, qtype: u16, rules: &Filter) -> Plan {
    match qtype {
        A | AAAA if endpoint_name(name, rules) => Plan::Own,
        A | AAAA if hosts_allow(name, rules) => Plan::Public,
        // A service's records are its host's: `_x._tcp.a.com` is a.com's, which
        // the hosts must allow, whatever a wildcard over it would.
        // Never a local network's own names, which this machine's
        // resolver answers from that network (mDNS, a company's zone).
        SRV | TXT if !public_name(service_of(name)) => {
            Plan::Refuse("refused: not a name on the internet")
        }
        SRV | TXT if hosts_allow(service_of(name), rules) => Plan::Ask,
        SRV | TXT => Plan::Refuse("refused: not a host this sandbox may reach"),
        _ if hosts_allow(name, rules) => Plan::Empty,
        _ => Plan::Refuse("refused: not a name this sandbox may reach"),
    }
}

/// The answer to `msg`, a query from the sandbox, over UDP or not; None:
/// none (not a query). Logged on `rules`' wire.
pub async fn respond(
    msg: &[u8],
    rules: Filter,
    servers: &[SocketAddr],
    udp: bool,
) -> Option<Vec<u8>> {
    let query = match parse_query(msg) {
        Ok(query) => query,
        Err(None) => return None,
        Err(Some((id, rcode))) => return Some(error(id, rcode)),
    };
    let mut call = crate::wire::Call::on(rules.wire);
    call.verb = Some("dns");
    call.to = format!("{} {}", query.name, type_name(query.qtype));
    let refuse = |call: &mut crate::wire::Call, why: String| {
        // The policy at work, as a 429 is: quiet, and never a pause.
        call.limited = true;
        call.end(&Err(anyhow::anyhow!("refused: {why}")));
        Some(answer(&query, REFUSED, &[], udp))
    };
    if let Some(why) = rules.wire.and_then(|w| w.why_stopped()) {
        return refuse(&mut call, why);
    }
    let plan = plan(&query.name, query.qtype, &rules);
    // What asks outside counts toward its calls a minute: a name's public
    // addresses once a lookup (its A or its AAAA, whichever asks first;
    // the other waits for its answer, or finds it kept, as a retry of a
    // name with none does), never the sandbox's own answers.
    let counted = matches!(plan, Plan::Ask)
        || (plan == Plan::Public && !link_sandbox_route::resolve::is_resolved(&query.name));
    if counted && let Some((_, why)) = rules.wire.and_then(|w| w.over_limit()) {
        return refuse(&mut call, why);
    }
    let found: anyhow::Result<(u8, Vec<(Record, u32)>)> = match plan {
        Plan::Refuse(why) => return refuse(&mut call, why.into()),
        Plan::Empty => Ok((NOERROR, Vec::new())),
        Plan::Own if query.qtype == AAAA => Ok((NOERROR, Vec::new())),
        Plan::Own => {
            let addr = NAMES
                .lock()
                .ok()
                .and_then(|mut names| names.get_or_insert_default().address(&query.name));
            match addr {
                Some(addr) => Ok((NOERROR, vec![(Record::A(addr), 60)])),
                None => Err(anyhow::anyhow!(
                    "every address of the sandbox's own is given"
                )),
            }
        }
        Plan::Public => public(&query.name, query.qtype, &rules).await,
        Plan::Ask => {
            let asked = async {
                let _turn = LOOKUPS.acquire().await;
                ask(servers, &query.name, query.qtype).await
            };
            match tokio::time::timeout(ASK_TIMEOUT, asked).await {
                Ok(found) => found,
                Err(_) => Err(anyhow::anyhow!("{}: no answer in time", query.name)),
            }
        }
    };
    match found {
        Ok((rcode, records)) => {
            call.end(&Ok(()));
            Some(answer(&query, rcode, &records, udp))
        }
        Err(err) => {
            let rcode = match err.is::<crate::wire::Metadata>() {
                true => REFUSED,
                false => SERVFAIL,
            };
            call.metadata = rcode == REFUSED;
            call.end(&Err(err));
            Some(answer(&query, rcode, &[], udp))
        }
    }
}
