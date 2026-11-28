//! Each agent's block of ports, kept for its harness, chosen free of the
//! others' and this machine's.

use super::*;
use link_format::WithinExt;

/// The ports agent `agent`'s harness serves, as its last start's
/// `Launch` said.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ports {
    harness: String,
    ports: Vec<u16>,
}

link_files::specification!(Ports, include_str!("ports.schema.toml"));

/// Each agent's ports, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Ports>()
}

/// Keeps the ports harness `harness` of agent `agent` serves, with its
/// settings, where no harness reaches: `lnk agent show` checks its
/// health without asking its adapter again.
pub fn save_ports(agent: &str, harness: &str, ports: &[u16]) -> anyhow::Result<()> {
    let path = config_of(agent).join("ports.json");
    let ports = Ports {
        harness: harness.to_string(),
        ports: ports.to_vec(),
    };
    link_files::write_private(&path, &link_files::saved::print_json(&ports)?)
        .within(format!("writing {}", path.display()))
}

/// The ports [`save_ports`] kept for harness `harness` of agent `agent`.
pub fn ports_of(agent: &str, harness: &str) -> Option<Vec<u16>> {
    let bytes = std::fs::read(config_of(agent).join("ports.json")).ok()?;
    let saved: Ports = link_files::saved::parse_json(&bytes).ok()?;
    (saved.harness == harness).then_some(saved.ports)
}

/// Where agents' blocks of ports start, unless this machine says
/// otherwise ([`first_port`]).
pub const FIRST_PORT: u16 = 20000;

/// Where agents' blocks of ports start on this machine: its agent
/// policy's `ports_from`, else [`FIRST_PORT`].
pub fn first_port() -> anyhow::Result<u16> {
    crate::machine_settings::ports_from()
}

/// The most ports an agent's block holds.
pub const BLOCK_MAX: u16 = 1000;

/// How many ports an agent whose harness answered `contract` takes from
/// its base port: the harness's ([`link_harness_contract::harness::Contract::ports`]),
/// and one after them for its proxy on a Mac; [`PORT_BLOCK`] for an
/// adapter that doesn't say.
pub fn block_for(contract: &link_harness_contract::harness::Contract) -> u16 {
    match contract.ports {
        Some(ports) => ports.clamp(1, BLOCK_MAX - 1) + 1,
        None => PORT_BLOCK,
    }
}

/// The blocks of ports every other agent here has: each base port, and
/// how many from it.
pub(super) fn others_ports(name: &str) -> anyhow::Result<Vec<(u16, u16)>> {
    let mut taken = Vec::new();
    for other in names()?.iter().filter(|other| *other != name) {
        let peek = peek(other)?;
        if let Some(port) = peek.port {
            taken.push((port, peek.block.unwrap_or(PORT_BLOCK)));
        }
        if let Some(port) = peek.webhook_port {
            taken.push((port, 1));
        }
    }
    Ok(taken)
}

/// A port for agent `agent`'s webhooks: free here, outside its own
/// block and saved webhook port, and every other agent's.
pub fn free_service_port(agent: &Settings) -> anyhow::Result<u16> {
    let mut taken = others_ports(&agent.name)?;
    if let Some(port) = agent.port {
        taken.push((port, agent.block()));
    }
    taken.extend(agent.webhook_port.map(|port| (port, 1)));
    random_free_block(first_port()?, &taken, 1)
        .within("no port left for this agent's services here")
}

/// Whether `port` is taken on this machine's loopback, IPv4's or IPv6's:
/// another program listens there.
pub fn port_taken(port: u16) -> bool {
    [
        std::net::IpAddr::from(std::net::Ipv4Addr::LOCALHOST),
        std::net::Ipv6Addr::LOCALHOST.into(),
    ]
    .into_iter()
    .any(|ip| {
        matches!(
            std::net::TcpListener::bind((ip, port)),
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse
        )
    })
}

/// Whether a program listens on `port` on this machine's loopback, IPv4's
/// or IPv6's: a connection there is taken. A port only held by a
/// connection out of this machine, or one closing, is not.
pub fn listened_on(port: u16) -> bool {
    [
        std::net::IpAddr::from(std::net::Ipv4Addr::LOCALHOST),
        std::net::Ipv6Addr::LOCALHOST.into(),
    ]
    .into_iter()
    .any(|ip| {
        std::net::TcpStream::connect_timeout(
            &(ip, port).into(),
            std::time::Duration::from_millis(200),
        )
        .is_ok()
    })
}

/// A block of `size` ports for agent `name` that no other agent here has
/// and no program on this machine holds: every agent's, `main`'s too,
/// when it's made or arrives, and where it moves when another program,
/// another user's agent say, holds its own. Taken from a random place
/// among the free blocks below the system's own range for outgoing
/// connections ([`EPHEMERAL`]), whose ports any program may be given at
/// any moment, so two users of the machine making or moving agents at
/// once rarely pick the same; from above it only when none is free
/// below.
pub fn free_block_here(name: &str, size: u16) -> anyhow::Result<u16> {
    random_free_block(first_port()?, &others_ports(name)?, size)
        .within("no ports left for this agent on this machine")
}

/// A block of `size` ports from `from` none of `taken` overlaps and no
/// program here holds, at a random place below [`EPHEMERAL`], else above
/// it. Any port below may be its first, so a block made long and shrunk
/// at its next start, as a new agent's is, still lands anywhere, and the
/// candidates are visited from a random one by a random step, so two
/// picks at once seldom meet, even beside blocks already taken.
pub(super) fn random_free_block(from: u16, taken: &[(u16, u16)], size: u16) -> Option<u16> {
    let size = size.max(1);
    let last = |to: u32| to.saturating_sub(u32::from(size));
    let below: Vec<u16> = (u32::from(from)..=last(u32::from(EPHEMERAL)))
        .filter_map(|base| u16::try_from(base).ok())
        .collect();
    let above = (u32::from(EPHEMERAL)..=last(u32::from(u16::MAX) + 1))
        .step_by(usize::from(size))
        .filter_map(|base| u16::try_from(base).ok());
    let count = below.len() as u64;
    let start = u64::from(random_u32()) % count.max(1);
    let step = coprime(u64::from(random_u32()), count);
    (0..count)
        .map(|k| below[((start + k * step) % count) as usize])
        .chain(above)
        .find(|&base| fits(from, base, size, taken) && free_here(base, size))
}

/// A step from 1 that shares no factor with `count`, near `seed`: from
/// any start, it visits each of `count` places once.
pub(super) fn coprime(seed: u64, count: u64) -> u64 {
    let gcd = |mut a: u64, mut b: u64| {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    };
    let mut step = seed % count.max(1) + 1;
    while gcd(step, count) > 1 {
        step += 1;
    }
    step
}

/// Where the system's own range for outgoing connections starts, on Linux
/// (`ip_local_port_range`'s usual first) and below macOS's (49152): an
/// agent's ports are placed below it where they can be.
pub const EPHEMERAL: u16 = 32768;

/// Whether no program on this machine holds any of `size` ports from
/// `base`.
fn free_here(base: u16, size: u16) -> bool {
    (base..base.saturating_add(size)).all(|port| !port_taken(port))
}

/// A random number, from the system's randomness.
fn random_u32() -> u32 {
    link_harness_contract::harness::random_token()
        .ok()
        .and_then(|token| u32::from_str_radix(token.get(..8)?, 16).ok())
        .unwrap_or(0)
}

/// Whether `size` ports from `base`, all from `from` (this machine's
/// [`first_port`]), overlap none of `taken`.
pub(super) fn fits(from: u16, base: u16, size: u16, taken: &[(u16, u16)]) -> bool {
    let end = u32::from(base) + u32::from(size);
    base >= from
        && end <= u32::from(u16::MAX) + 1
        && taken.iter().all(|(t, n)| {
            let t_end = u32::from(*t) + u32::from(*n);
            t_end <= u32::from(base) || end <= u32::from(*t)
        })
}
