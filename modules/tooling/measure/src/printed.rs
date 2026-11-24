//! What a Mac's commands print of its network and CPU, read: each
//! interface's bytes (`netstat -ibn`), the TCP ports listened on
//! (`netstat -anp tcp`) and by which of this user's processes (`lsof`),
//! and the CPU's ticks. Built on every system, so read in tests against
//! output recorded on a Mac; run only there (`crate::mac`).

use std::collections::{BTreeMap, BTreeSet};

/// Each interface's bytes in and out, from `netstat -ibn`: its link's
/// row (`<Link#n>`), which counts all of its addresses' together, in
/// 64 bits. Only interfaces on a device (`en0`, `en1`): a tunnel's
/// (`utun`), a bridge's or the loopback's traffic is a device's too,
/// or never leaves the machine.
pub fn netstat_interfaces(text: &str) -> BTreeMap<String, (u64, u64)> {
    let mut found = BTreeMap::new();
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return found;
    };
    let header: Vec<&str> = header.split_whitespace().collect();
    // Counted from the end: an interface without an address (the
    // loopback's link) has one column fewer before them.
    let from_end = |name: &str| {
        header
            .iter()
            .position(|h| *h == name)
            .map(|i| header.len() - i)
    };
    let (Some(received), Some(sent)) = (from_end("Ibytes"), from_end("Obytes")) else {
        return found;
    };
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < received || !fields.get(2).is_some_and(|n| n.starts_with("<Link#")) {
            continue;
        }
        let name = fields[0].trim_end_matches('*');
        let device = name
            .strip_prefix("en")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !device {
            continue;
        }
        let at = |from_end: usize| fields[fields.len() - from_end].parse::<u64>().ok();
        if let (Some(i), Some(o)) = (at(received), at(sent)) {
            found.insert(name.to_string(), (i, o));
        }
    }
    found
}

/// The port of an address as `netstat` writes it (`127.0.0.1.8080`,
/// `*.5000`, `::1.631`) or `lsof` (`*:7000`, `[::1]:18800`).
fn port(address: &str, separator: char) -> Option<u16> {
    address.rsplit_once(separator)?.1.parse().ok()
}

/// The TCP ports listened on, every user's, from `netstat -anp tcp`.
pub fn netstat_listening(text: &str) -> BTreeSet<u16> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let listening = fields.first().is_some_and(|p| p.starts_with("tcp"))
                && fields.last() == Some(&"LISTEN");
            listening.then(|| port(fields.get(3)?, '.')).flatten()
        })
        .collect()
}

/// The TCP ports each process listens on, by pid, from `lsof -nP -w
/// -iTCP -sTCP:LISTEN -Fpn`: a `p` line a process, an `n` line an
/// address it listens on.
pub fn lsof_listening(text: &str) -> BTreeMap<u32, BTreeSet<u16>> {
    let mut found: BTreeMap<u32, BTreeSet<u16>> = BTreeMap::new();
    let mut pid = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse::<u32>().ok();
        } else if let Some(address) = line.strip_prefix('n')
            && let (Some(pid), Some(port)) = (pid, port(address, ':'))
        {
            found.entry(pid).or_default().insert(port);
        }
    }
    found
}

/// The CPU the machine has used, in core-seconds, from its ticks as
/// `host_statistics` counts them (user, system, idle, nice), every core's
/// together, `hz` a second: all but idle.
pub fn cpu_used(ticks: [u32; 4], hz: f64) -> f64 {
    let [user, system, _idle, nice] = ticks;
    (u64::from(user) + u64::from(system) + u64::from(nice)) as f64 / hz
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `netstat -ibn` on a Mac, as recorded: the loopback, a utun, a
    /// tunnel that's down, Wi-Fi with two addresses, a bridge.
    const NETSTAT_IBN: &str = "\
Name       Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll
lo0        16384 <Link#1>                          884331     0  371120458   884331     0  371120458     0
lo0        16384 127           127.0.0.1           884331     -  371120458   884331     -  371120458     -
gif0*      1280  <Link#2>                               0     0          0        0     0          0     0
en0        1500  <Link#11>   3c:22:fb:12:34:56    5025481     0 6236521354  2147483     0  351234567     0
en0        1500  fe80::1c2:3 fe80:b::1c2:3e4f:5   5025481     - 6236521354  2147483     -  351234567     -
en0        1500  192.168.1     192.168.1.23       5025481     - 6236521354  2147483     -  351234567     -
en5*       1500  <Link#14>   ac:de:48:00:11:22         10     0       4000       12     0       5000     0
bridge0    1500  <Link#16>   36:5a:1b:aa:bb:cc          0     0          0        3     0        300     0
utun3      1380  <Link#20>                          10421     0    9876543     9120     0    1234567     0
";

    #[test]
    fn a_macs_interfaces_are_read_from_netstat() {
        let found = netstat_interfaces(NETSTAT_IBN);
        assert_eq!(
            found,
            BTreeMap::from([
                ("en0".to_string(), (6_236_521_354, 351_234_567)),
                ("en5".to_string(), (4000, 5000)),
            ])
        );
        assert!(netstat_interfaces("").is_empty());
        assert!(netstat_interfaces("Name Mtu\nen0 1500 <Link#1> 1 2\n").is_empty());
    }

    /// `netstat -anp tcp` on a Mac, as recorded.
    #[test]
    fn a_macs_listening_ports_are_read_from_netstat() {
        let text = "\
Active Internet connections (including servers)
Proto Recv-Q Send-Q  Local Address          Foreign Address        (state)
tcp4       0      0  192.168.1.23.55123     17.57.144.10.5223      ESTABLISHED
tcp4       0      0  127.0.0.1.8080         *.*                    LISTEN
tcp46      0      0  *.5000                 *.*                    LISTEN
tcp6       0      0  ::1.631                *.*                    LISTEN
tcp4       0      0  127.0.0.1.631          *.*                    LISTEN
";
        assert_eq!(netstat_listening(text), BTreeSet::from([631, 5000, 8080]));
    }

    /// `lsof -nP -w -iTCP -sTCP:LISTEN -Fpn` on a Mac, as recorded.
    #[test]
    fn each_processs_listening_ports_are_read_from_lsof() {
        let text =
            "p512\nf5\nn*:7000\nf6\nn*:5000\np3001\nf20\nn127.0.0.1:18800\nf21\nn[::1]:18800\n";
        let found = lsof_listening(text);
        assert_eq!(found[&512], BTreeSet::from([5000, 7000]));
        assert_eq!(found[&3001], BTreeSet::from([18800]));
        assert!(lsof_listening("").is_empty());
    }

    #[test]
    fn a_macs_cpu_is_its_ticks_but_idle() {
        assert_eq!(cpu_used([1000, 500, 90_000, 100], 100.0), 16.0);
        // Each count is 32 bits; their sum isn't.
        assert_eq!(
            cpu_used([u32::MAX, u32::MAX, 0, 0], 1.0),
            2.0 * f64::from(u32::MAX)
        );
    }
}
