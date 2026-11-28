//! `[lan]`: the machines of your network a sandbox may reach through its
//! proxy. Only private addresses (10/8, 172.16/12, 192.168/16, fc00::/7),
//! and the carrier-grade range (100.64/10) only by an exact address;
//! never this machine (its containers' and VMs' bridges too), a
//! link-local address or a cloud's metadata service, whatever is listed.
//! An IPv4 address written in IPv6 is judged as that IPv4 address.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use anyhow::ensure;

/// What `[lan]` counts as this machine's own ([`lan.toml`]).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Own {
    /// The start of each name of an interface whose network is this
    /// machine's: its containers' and virtual machines'.
    interfaces: Vec<String>,
    /// Files only a cloud's Mac has.
    cloud_mac_files: Vec<std::path::PathBuf>,
}

/// [`Own`], read once.
fn own() -> &'static Own {
    static OWN: std::sync::OnceLock<Own> = std::sync::OnceLock::new();
    OWN.get_or_init(|| toml::from_str(include_str!("lan.toml")).expect("lan.toml reads"))
}

/// Whether `host` may be reached as one of your network's machines at
/// all, before it is resolved: `*`, its name or address listed, or an
/// address in a listed range.
pub fn may(host: &str, entries: &[String]) -> bool {
    let host = host.trim_end_matches('.');
    entries.iter().any(|entry| {
        entry == "*"
            || entry.eq_ignore_ascii_case(host)
            || host_address(host).is_some_and(|ip| matches(entry, ip))
    })
}

/// `host` as an address, IPv4 in any form a URL may write it (`3232235777`
/// is `192.168.1.1`), as the resolver reads it too.
fn host_address(host: &str) -> Option<IpAddr> {
    host.parse::<IpAddr>().ok().or_else(|| {
        crate::http::ends_in_number(host)
            .then(|| crate::http::url_ipv4(host).map(IpAddr::V4))
            .flatten()
    })
}

/// Whether `ip`, one of `host`'s addresses, may be reached as a machine
/// of your network by `entries`.
pub fn allowed(ip: IpAddr, host: &str, entries: &[String]) -> bool {
    let ip = canonical(ip);
    let exact = entries
        .iter()
        .any(|e| e.parse::<IpAddr>().ok().map(canonical) == Some(ip));
    if !private(ip) && !(carrier_grade(ip) && exact) {
        return false;
    }
    let host = host.trim_end_matches('.');
    let listed = entries.iter().any(|entry| {
        entry == "*" && private(ip) || entry.eq_ignore_ascii_case(host) || matches(entry, ip)
    });
    // Last, as it looks at this machine's interfaces: an internet
    // address, or one `[lan]` doesn't list, never gets this far.
    listed && !never(ip)
}

/// Never reached through `[lan]`: loopback, link-local (a cloud's
/// metadata service among them), and this machine's own addresses, its
/// bridges' networks among them.
fn never(ip: IpAddr) -> bool {
    let local = match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // Local NAT64 (64:ff9b:1::/48): where its IPv4 address
                // sits depends on the network's prefix.
                || v6.segments()[..3] == [0x64, 0xff9b, 1]
        }
    };
    local
        || crate::wire::is_metadata(ip)
        || link_sandbox_contract::net::this_machine(ip)
        || bridged(ip)
}

/// Whether `ip` is on the network of one of this machine's bridges or
/// virtual machines' interfaces ([`is_this_machines`]): its containers
/// and VMs, which are this machine.
fn bridged(ip: IpAddr) -> bool {
    bridges()
        .iter()
        .any(|(addr, mask)| same_network(ip, *addr, *mask))
}

/// Whether `ip` is on the network of `addr` with netmask `mask`.
fn same_network(ip: IpAddr, addr: IpAddr, mask: IpAddr) -> bool {
    match (ip, addr, mask) {
        (IpAddr::V4(ip), IpAddr::V4(addr), IpAddr::V4(mask)) => {
            let mask = u32::from(mask);
            u32::from(ip) & mask == u32::from(addr) & mask
        }
        (IpAddr::V6(ip), IpAddr::V6(addr), IpAddr::V6(mask)) => {
            let mask = u128::from(mask);
            u128::from(ip) & mask == u128::from(addr) & mask
        }
        _ => false,
    }
}

/// Each address of this machine's bridges, with its netmask.
#[cfg(unix)]
fn bridges() -> Vec<(IpAddr, IpAddr)> {
    let mut found = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `list`, which is read as the C library
    // lays it out and freed once, below.
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return found;
    }
    let mut at = list;
    while !at.is_null() {
        // SAFETY: `at` is a node of the list getifaddrs made.
        let ifa = unsafe { &*at };
        at = ifa.ifa_next;
        // SAFETY: a name getifaddrs gave, NUL-terminated.
        let name = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) };
        let name = name.to_string_lossy();
        if !is_this_machines(&name) {
            continue;
        }
        if let (Some(addr), Some(mask)) = (address(ifa.ifa_addr), address(ifa.ifa_netmask)) {
            found.push((addr, mask));
        }
    }
    // SAFETY: the list getifaddrs made, freed once.
    unsafe { libc::freeifaddrs(list) };
    found
}

#[cfg(not(unix))]
fn bridges() -> Vec<(IpAddr, IpAddr)> {
    Vec::new()
}

/// Whether the interface `name`'s network is this machine's own: one
/// named as its containers' and VMs' are ([`lan.toml`]), and on Linux a
/// bridge, as the kernel says, or any interface with no device behind
/// it, read in `/sys/class/net`.
fn is_this_machines(name: &str) -> bool {
    is_this_machines_in(name, std::path::Path::new("/sys/class/net"))
}

/// [`is_this_machines`], Linux's interfaces read in `net`.
fn is_this_machines_in(name: &str, net: &std::path::Path) -> bool {
    if name.is_empty() || name.contains('/') || name == "lo" {
        return false;
    }
    if own()
        .interfaces
        .iter()
        .any(|start| name.starts_with(start.as_str()))
    {
        return true;
    }
    if cfg!(target_os = "macos") {
        return false;
    }
    let at = net.join(name);
    at.is_dir() && (at.join("bridge").is_dir() || !at.join("device").exists())
}

/// The IP address in `sa`, if it holds one.
#[cfg(unix)]
fn address(sa: *const libc::sockaddr) -> Option<IpAddr> {
    if sa.is_null() {
        return None;
    }
    // SAFETY: a sockaddr getifaddrs gave, read as the family it says.
    unsafe {
        match i32::from((*sa).sa_family) {
            libc::AF_INET => {
                let v4 = &*(sa as *const libc::sockaddr_in);
                Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(v4.sin_addr.s_addr))))
            }
            libc::AF_INET6 => {
                let v6 = &*(sa as *const libc::sockaddr_in6);
                Some(IpAddr::V6(Ipv6Addr::from(v6.sin6_addr.s6_addr)))
            }
            _ => None,
        }
    }
}

/// Whether `entry`, a range, holds the whole of a private block (10/8,
/// 172.16/12, 192.168/16, fc00::/7): `0.0.0.0/0`, `10.0.0.0/8`,
/// `::/0`, as much as `*` of that block. A range is a base and its bits;
/// it holds a block when it has as few bits or fewer and their first
/// bits agree.
pub fn whole_private_block(entry: &str) -> bool {
    let Some((base, bits)) = entry.split_once('/') else {
        return false;
    };
    let (Ok(base), Ok(bits)) = (base.parse::<IpAddr>(), bits.parse::<u32>()) else {
        return false;
    };
    let blocks: [(IpAddr, u32); 4] = [
        (Ipv4Addr::new(10, 0, 0, 0).into(), 8),
        (Ipv4Addr::new(172, 16, 0, 0).into(), 12),
        (Ipv4Addr::new(192, 168, 0, 0).into(), 16),
        (
            "fc00::"
                .parse::<Ipv6Addr>()
                .unwrap_or(Ipv6Addr::UNSPECIFIED)
                .into(),
            7,
        ),
    ];
    blocks.iter().any(|(block, block_bits)| {
        bits <= *block_bits
            && match (canonical(base), block) {
                (IpAddr::V4(base), IpAddr::V4(block)) => {
                    let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
                    u32::from(base) & mask == u32::from(*block) & mask
                }
                (IpAddr::V6(base), IpAddr::V6(block)) => {
                    let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
                    u128::from(base) & mask == u128::from(*block) & mask
                }
                _ => false,
            }
    })
}

/// The range `entry` (an address, or one with `/<bits>`) writes: its
/// family (true for IPv4) and its first and last address, IPv4 as
/// IPv4 however written.
fn range(entry: &str) -> Option<(bool, u128, u128)> {
    let (base, bits) = match entry.split_once('/') {
        Some((base, bits)) => (
            base.parse::<IpAddr>().ok()?,
            Some(bits.parse::<u32>().ok()?),
        ),
        None => (entry.parse::<IpAddr>().ok()?, None),
    };
    match canonical(base) {
        IpAddr::V4(v4) => {
            let bits = bits.unwrap_or(32);
            if bits > 32 {
                return None;
            }
            let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
            let start = u32::from(v4) & mask;
            Some((true, start.into(), (start | !mask).into()))
        }
        IpAddr::V6(v6) => {
            let bits = bits.unwrap_or(128);
            if bits > 128 {
                return None;
            }
            let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
            let start = u128::from(v6) & mask;
            Some((false, start, start | !mask))
        }
    }
}

/// Whether `entries` together hold a whole private block (10/8,
/// 172.16/12, 192.168/16, fc00::/7), however many ranges they split it
/// into (`10.0.0.0/9` and `10.128.0.0/9`).
pub fn whole_private_blocks(entries: &[String]) -> bool {
    let ranges: Vec<(bool, u128, u128)> = entries.iter().filter_map(|e| range(e)).collect();
    ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "fc00::/7"]
        .iter()
        .filter_map(|block| range(block))
        .any(|(v4, first, last)| {
            let mut parts: Vec<(u128, u128)> = ranges
                .iter()
                .filter(|(family, start, end)| *family == v4 && *end >= first && *start <= last)
                .map(|(_, start, end)| ((*start).max(first), (*end).min(last)))
                .collect();
            parts.sort();
            let mut next = first;
            for (start, end) in parts {
                if start > next {
                    return false;
                }
                if end >= last {
                    return true;
                }
                next = next.max(end + 1);
            }
            false
        })
}

/// 10/8, 172.16/12, 192.168/16, fc00::/7.
fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private(),
        IpAddr::V6(v6) => (v6.segments()[0] & 0xfe00) == 0xfc00,
    }
}

/// 100.64/10: carrier-grade NAT, Tailscale.
fn carrier_grade(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            a == 100 && (64..128).contains(&b)
        }
        IpAddr::V6(_) => false,
    }
}

/// An IPv4 address written as IPv6 as itself: mapped (`::ffff:a.b.c.d`),
/// compatible (`::a.b.c.d`), NAT64 (`64:ff9b::a.b.c.d`) and 6to4
/// (`2002:<v4>::`), each of which reaches that IPv4 address.
pub fn canonical(ip: IpAddr) -> IpAddr {
    let IpAddr::V6(v6) = ip else {
        return ip;
    };
    if let Some(v4) = v6.to_ipv4_mapped() {
        return IpAddr::V4(v4);
    }
    let s = v6.segments();
    let v4 = |hi: u16, lo: u16| {
        let [a, b] = hi.to_be_bytes();
        let [c, d] = lo.to_be_bytes();
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    };
    match s {
        [0, 0, 0, 0, 0, 0, ..] if !v6.is_loopback() && !v6.is_unspecified() => v4(s[6], s[7]),
        [0x64, 0xff9b, 0, 0, 0, 0, ..] => v4(s[6], s[7]),
        [0x2002, ..] => v4(s[1], s[2]),
        _ if NAT64
            .get()
            .is_some_and(|prefixes| prefixes.iter().any(|p| p[..] == s[..6])) =>
        {
            v4(s[6], s[7])
        }
        _ => ip,
    }
}

/// This network's own NAT64 prefixes (`/96`), learned from
/// `ipv4only.arpa` ([`learn_nat64`]): an address in one is an IPv4
/// address, judged as that one.
static NAT64: tokio::sync::OnceCell<Vec<[u16; 6]>> = tokio::sync::OnceCell::const_new();

/// How long learning this network's NAT64 prefix may take, once.
const NAT64_LOOKUP: std::time::Duration = std::time::Duration::from_secs(2);

/// Learns this network's own NAT64 prefixes once (RFC 7050): the IPv6
/// addresses this machine's resolver gives `ipv4only.arpa`, which has
/// only the IPv4 addresses 192.0.0.170 and 192.0.0.171, each that
/// address under its network's prefix. None, or no answer in time,
/// learns none: the well-known prefix alone counts.
pub async fn learn_nat64() {
    NAT64
        .get_or_init(|| async {
            let found =
                tokio::time::timeout(NAT64_LOOKUP, tokio::net::lookup_host(("ipv4only.arpa", 0)))
                    .await;
            match found {
                Ok(Ok(addrs)) => nat64_prefixes(addrs.map(|a| a.ip())),
                _ => Vec::new(),
            }
        })
        .await;
}

/// The `/96` NAT64 prefixes `addrs`, `ipv4only.arpa`'s addresses, show:
/// each IPv6 address ending in 192.0.0.170 or 192.0.0.171.
fn nat64_prefixes(addrs: impl Iterator<Item = IpAddr>) -> Vec<[u16; 6]> {
    let mut prefixes: Vec<[u16; 6]> = Vec::new();
    for addr in addrs {
        let IpAddr::V6(v6) = addr else {
            continue;
        };
        let s = v6.segments();
        let known = [Ipv4Addr::new(192, 0, 0, 170), Ipv4Addr::new(192, 0, 0, 171)];
        let embedded = Ipv4Addr::from((u32::from(s[6]) << 16) | u32::from(s[7]));
        let mut prefix = [0u16; 6];
        prefix.copy_from_slice(&s[..6]);
        if known.contains(&embedded) && !prefixes.contains(&prefix) {
            prefixes.push(prefix);
        }
    }
    prefixes
}

/// Whether `entry`, an address or a range, holds `ip`.
fn matches(entry: &str, ip: IpAddr) -> bool {
    let ip = canonical(ip);
    if let Ok(one) = entry.parse::<IpAddr>() {
        return canonical(one) == ip;
    }
    let Some((base, bits)) = entry.split_once('/') else {
        return false;
    };
    let (Ok(base), Ok(bits)) = (base.parse::<IpAddr>(), bits.parse::<u32>()) else {
        return false;
    };
    match (canonical(base), ip) {
        (IpAddr::V4(base), IpAddr::V4(ip)) if bits <= 32 => {
            let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
            u32::from(base) & mask == u32::from(ip) & mask
        }
        (IpAddr::V6(base), IpAddr::V6(ip)) if bits <= 128 => {
            let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
            u128::from(base) & mask == u128::from(ip) & mask
        }
        _ => false,
    }
}

/// Whether this is a cloud's machine (AWS, Google Cloud, Azure, as its
/// firmware says), where "your network" is the cloud's. `LNK_TEST_ON_CLOUD=1`
/// makes it one, for tests; nothing makes a cloud's machine not one.
pub fn on_cloud() -> bool {
    std::env::var("LNK_TEST_ON_CLOUD").is_ok_and(|v| v == "1")
        || match cfg!(target_os = "macos") {
            true => mac_on_cloud_at(std::path::Path::new("/")),
            false => on_cloud_at(std::path::Path::new("/sys")),
        }
}

/// [`on_cloud`] on a Mac whose `/` is at `root`: an EC2 Mac, by the files
/// only a cloud's Mac has ([`lan.toml`]).
pub fn mac_on_cloud_at(root: &std::path::Path) -> bool {
    own()
        .cloud_mac_files
        .iter()
        .any(|file| root.join(file.strip_prefix("/").unwrap_or(file)).exists())
}

/// [`on_cloud`], the machine's `/sys` at `sys`: AWS's firmware names
/// Amazon (on its Xen machines, only in the BIOS's version, and the
/// hypervisor's id starts `ec2`), Google Cloud's is Google's Compute
/// Engine, and Azure's chassis carries Azure's asset tag.
pub fn on_cloud_at(sys: &std::path::Path) -> bool {
    let read = |file: &str| {
        std::fs::read_to_string(sys.join(file))
            .map(|text| text.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };
    let dmi = |file: &str| read(&format!("class/dmi/id/{file}"));
    let aws = [
        "sys_vendor",
        "product_name",
        "bios_vendor",
        "bios_version",
        "board_vendor",
    ]
    .iter()
    .any(|file| dmi(file).contains("amazon"))
        || read("hypervisor/uuid").starts_with("ec2");
    let google =
        dmi("product_name").contains("google compute engine") || dmi("bios_vendor") == "google";
    let azure = dmi("chassis_asset_tag") == "7783-7084-3265-9085";
    aws || google || azure
}

/// An error if `lan` reaches a whole private block on a cloud's machine
/// (`cloud`): `"*"`, or a range holding one (`0.0.0.0/0`, `10.0.0.0/8`).
/// Its private ranges are the cloud's network, other people's machines
/// among them, so `[lan]` lists addresses there.
pub fn lan_here(lan: &[String], cloud: bool) -> anyhow::Result<()> {
    if !cloud {
        return Ok(());
    }
    for entry in lan {
        ensure!(
            entry != "*" && !whole_private_block(entry),
            "[lan] hosts = [{entry:?}] isn't taken on a cloud's machine, where the network \
             is the cloud's: list the addresses"
        );
    }
    ensure!(
        !whole_private_blocks(lan),
        "[lan] hosts = {lan:?} together hold a whole private block, which isn't taken on a \
         cloud's machine, where the network is the cloud's: list the addresses"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|e| e.to_string()).collect()
    }

    /// This machine's virtual machines' and containers' interfaces are
    /// this machine's, by name from `lan.toml`, and on Linux any
    /// interface with no device behind it; a real network card's isn't.
    #[test]
    fn this_machines_virtual_machines_are_this_machines() {
        let net = link_test_kit::TempDir::new("net");
        for vm in ["vboxnet0", "vmnet8", "vnic0", "tap3", "bridge100", "virbr0"] {
            assert!(is_this_machines_in(vm, &net), "{vm}");
        }
        std::fs::create_dir_all(net.join("eth0").join("device")).unwrap();
        std::fs::create_dir_all(net.join("veth9")).unwrap();
        std::fs::create_dir_all(net.join("en7").join("device")).unwrap();
        std::fs::create_dir_all(net.join("en7").join("bridge")).unwrap();
        assert!(
            !is_this_machines_in("eth0", &net),
            "a card's network is yours"
        );
        assert!(is_this_machines_in("en7", &net), "a bridge");
        assert_eq!(
            is_this_machines_in("veth9", &net),
            !cfg!(target_os = "macos"),
            "no device behind it"
        );
        assert!(!is_this_machines_in("lo", &net));
        assert!(!is_this_machines_in("../eth0", &net));
    }

    /// An EC2 Mac is a cloud's machine, by the files only it has.
    #[test]
    fn an_ec2_mac_is_a_clouds() {
        let root = link_test_kit::TempDir::new("mac");
        assert!(!mac_on_cloud_at(&root));
        let daemons = root.join("Library/LaunchDaemons");
        std::fs::create_dir_all(&daemons).unwrap();
        std::fs::write(daemons.join("com.amazon.ec2.macos-init.plist"), "").unwrap();
        assert!(mac_on_cloud_at(&root));
    }

    /// A network's own NAT64 prefix shows in `ipv4only.arpa`'s IPv6
    /// addresses, each 192.0.0.170 or .171 under it; nothing else is one.
    #[test]
    fn a_networks_own_nat64_prefix_is_learned() {
        let addrs = [
            "2001:db8:64::c000:aa",
            "2001:db8:64::c000:ab",
            "192.0.0.170",
            "2001:db8:1::1",
        ]
        .map(|a| a.parse::<IpAddr>().unwrap());
        assert_eq!(
            nat64_prefixes(addrs.into_iter()),
            [[0x2001, 0xdb8, 0x64, 0, 0, 0]]
        );
    }

    #[test]
    fn star_is_the_private_ranges_only() {
        let star = list(&["*"]);
        for ip in ["10.1.2.3", "172.16.0.9", "192.168.1.20", "fd12::1"] {
            assert!(allowed(ip.parse().unwrap(), ip, &star), "{ip}");
        }
        for ip in [
            "8.8.8.8",
            "127.0.0.1",
            "169.254.169.254",
            "169.254.1.1",
            "fd00:ec2::254",
            "fe80::1",
            "100.100.100.200",
            "::ffff:127.0.0.1",
        ] {
            assert!(!allowed(ip.parse().unwrap(), ip, &star), "{ip}");
        }
    }

    /// A cloud's metadata service, listed by its exact address or in a
    /// range holding it, is reached by no `[lan]` entry: Google Cloud's
    /// IPv6 one is a private address, Alibaba Cloud's a carrier-grade one.
    #[test]
    fn no_entry_reaches_a_metadata_service() {
        for (ip, entries) in [
            ("fd20:ce::254", &["fd20:ce::254"][..]),
            ("fd20:ce::254", &["fd20::/16"][..]),
            ("fd00:ec2::254", &["fd00:ec2::254"][..]),
            ("100.100.100.200", &["100.100.100.200"][..]),
        ] {
            assert!(
                !allowed(ip.parse().unwrap(), ip, &list(entries)),
                "{ip} by {entries:?}"
            );
        }
        let neighbour = list(&["fd20::/16"]);
        assert!(allowed("fd20:ce::1".parse().unwrap(), "x", &neighbour));
    }

    #[test]
    fn a_range_never_reaches_past_the_private_ones() {
        let wide = list(&["0.0.0.0/0", "::/0"]);
        assert!(!allowed("8.8.8.8".parse().unwrap(), "8.8.8.8", &wide));
        assert!(!allowed("169.254.169.254".parse().unwrap(), "x", &wide));
        assert!(allowed("192.168.1.5".parse().unwrap(), "x", &wide));
        let home = list(&["192.168.1.0/24"]);
        assert!(allowed("192.168.1.200".parse().unwrap(), "x", &home));
        assert!(!allowed("192.168.2.1".parse().unwrap(), "x", &home));
        assert!(may("192.168.1.7", &home) && !may("192.168.2.1", &home));
        // Written as a URL may write it, an address is still that address.
        assert!(may("3232235783", &home) && may("192.168.0x1.7", &home));
        assert!(!may("3232235777.1", &home) && !may("0xc0a80201", &home));
    }

    #[test]
    fn a_name_reaches_its_private_addresses_only() {
        let nas = list(&["nas.local"]);
        assert!(may("NAS.local.", &nas) && !may("other.local", &nas));
        assert!(allowed("192.168.1.9".parse().unwrap(), "nas.local", &nas));
        // A listed name pointing at the internet is no way out to it.
        assert!(!allowed(
            "93.184.216.34".parse().unwrap(),
            "nas.local",
            &nas
        ));
    }

    #[test]
    fn ipv4_written_in_ipv6_is_judged_as_that_ipv4() {
        let star = list(&["*", "0.0.0.0/0", "::/0"]);
        // Loopback, link-local, the metadata service, each written as
        // compatible, NAT64 and 6to4.
        for ip in [
            "::127.0.0.1",
            "64:ff9b::127.0.0.1",
            "64:ff9b::a9fe:a9fe",
            "2002:a9fe:a9fe::1",
            "2002:7f00:1::",
            "64:ff9b::169.254.1.1",
            "64:ff9b:1::a9fe:a9fe",
            "64:ff9b:1:a9fe:a9:fe00::",
        ] {
            assert!(!allowed(ip.parse().unwrap(), ip, &star), "{ip}");
        }
        // This machine, by an address of its own written as NAT64.
        let own = std::net::UdpSocket::bind("0.0.0.0:0")
            .and_then(|s| s.connect("192.0.2.1:9").map(|()| s))
            .and_then(|s| s.local_addr());
        if let Ok(std::net::SocketAddr::V4(own)) = own
            && own.ip().is_private()
        {
            let [a, b, c, d] = own.ip().octets();
            let nat64 = Ipv6Addr::new(
                0x64,
                0xff9b,
                0,
                0,
                0,
                0,
                u16::from_be_bytes([a, b]),
                u16::from_be_bytes([c, d]),
            );
            assert!(!allowed(IpAddr::V6(nat64), "x", &star), "{nat64}");
        }
        // A private address so written is that address, in its range.
        let home = list(&["192.168.1.0/24"]);
        for ip in [
            "64:ff9b::192.168.1.9",
            "2002:c0a8:109::",
            "::ffff:192.168.1.9",
        ] {
            assert!(allowed(ip.parse().unwrap(), ip, &home), "{ip}");
        }
        assert!(!allowed("2002:c0a8:209::".parse().unwrap(), "x", &home));
    }

    #[test]
    fn a_bridges_network_is_this_machines() {
        let (addr, mask) = (
            "172.17.0.1".parse().unwrap(),
            "255.255.0.0".parse().unwrap(),
        );
        assert!(same_network("172.17.0.5".parse().unwrap(), addr, mask));
        assert!(!same_network("172.18.0.5".parse().unwrap(), addr, mask));
        assert!(!same_network("fd00::1".parse().unwrap(), addr, mask));
        let (addr, mask) = ("fd00:1::1".parse().unwrap(), "ffff:ffff::".parse().unwrap());
        assert!(same_network("fd00:1::9".parse().unwrap(), addr, mask));
        // Each bridge here, its network refused even by its exact address.
        for (addr, _) in bridges() {
            let entry = list(&[&addr.to_string()]);
            assert!(!allowed(addr, "x", &entry), "{addr}");
        }
        assert!(!is_this_machines("../../etc"));
    }

    #[test]
    fn a_range_holding_a_whole_private_block_is_told() {
        for whole in [
            "0.0.0.0/0",
            "8.0.0.0/5",
            "10.0.0.0/8",
            "172.16.0.0/12",
            "172.0.0.0/8",
            "192.168.0.0/16",
            "::/0",
            "fc00::/7",
            "fc00::/6",
        ] {
            assert!(whole_private_block(whole), "{whole}");
        }
        for part in [
            "10.0.0.0/9",
            "172.16.0.0/13",
            "192.168.1.0/24",
            "fd00::/8",
            "11.0.0.0/8",
            "10.0.0.5",
            "nas.local",
            "*",
        ] {
            assert!(!whole_private_block(part), "{part}");
        }
    }

    #[test]
    fn carrier_grade_only_by_its_exact_address() {
        let ts = "100.101.102.103";
        assert!(allowed(ts.parse().unwrap(), ts, &list(&[ts])));
        assert!(!allowed(ts.parse().unwrap(), ts, &list(&["100.64.0.0/10"])));
        assert!(!allowed(ts.parse().unwrap(), ts, &list(&["*"])));
    }
}
