//! A host and port as a request names them, read as a browser reads
//! them.

use std::net::Ipv4Addr;

/// `host:port` (`[v6]:port`), with `default` when there's no port. A
/// host is a name or an address: letters, digits, `.`, `-` and `_`
/// (and `:` and `%` in brackets).
pub fn host_port(text: &str, default: Option<u16>) -> Option<(String, u16)> {
    let (host, port) = split_host_port(text, default)?;
    let v6 = |b: u8| b.is_ascii_hexdigit() || b":.%".contains(&b);
    let name = |b: u8| b.is_ascii_alphanumeric() || b"-._".contains(&b);
    let bracketed = text.starts_with('[');
    host.bytes()
        .all(|b| if bracketed { v6(b) || name(b) } else { name(b) })
        .then_some((host, port))
}

fn split_host_port(text: &str, default: Option<u16>) -> Option<(String, u16)> {
    if let Some(rest) = text.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(port) => port.parse().ok()?,
            None => default?,
        };
        return Some((host.to_string(), port));
    }
    match text.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => Some((host.to_string(), port.parse().ok()?)),
        None if !text.is_empty() => Some((text.to_string(), default?)),
        _ => None,
    }
    .filter(|(host, _)| !host.is_empty())
}

/// Whether a browser takes `host` as an IPv4 address (or refuses it as a
/// broken one): its last part, after one trailing dot, is decimal or
/// `0x` hex, as the WHATWG URL standard says.
pub fn ends_in_number(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    let last = host.rsplit('.').next().unwrap_or_default();
    let hex = last.strip_prefix("0x").or_else(|| last.strip_prefix("0X"));
    !last.is_empty()
        && (last.bytes().all(|b| b.is_ascii_digit())
            || hex.is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit())))
}

/// `host` as the IPv4 address a browser reads it as, by the WHATWG URL
/// standard: one to four parts, each decimal, octal (`0177`) or hex
/// (`0x7f`), the last filling the bytes left, and one trailing dot. So
/// `127.1`, `2130706433`, `0x7f000001` and `0177.0.0.1.` are each
/// `127.0.0.1`, and `0` is `0.0.0.0`.
pub fn url_ipv4(host: &str) -> Option<Ipv4Addr> {
    let host = host.strip_suffix('.').unwrap_or(host);
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() > 4 {
        return None;
    }
    let numbers = parts
        .iter()
        .map(|part| url_number(part))
        .collect::<Option<Vec<u64>>>()?;
    let (last, rest) = numbers.split_last()?;
    if rest.iter().any(|n| *n > 255) || *last >= 1 << (8 * (4 - rest.len())) {
        return None;
    }
    let ip = rest
        .iter()
        .enumerate()
        .fold(*last, |ip, (i, n)| ip + (n << (8 * (3 - i))));
    Some(Ipv4Addr::from(u32::try_from(ip).ok()?))
}

/// One part of an IPv4 address in a URL: `0x` hex (`0x` alone is 0), a
/// leading `0` octal, else decimal.
fn url_number(part: &str) -> Option<u64> {
    let (digits, radix) = match part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
        Some(hex) => (hex, 16),
        None if part.len() > 1 && part.starts_with('0') => (&part[1..], 8),
        None => (part, 10),
    };
    if part.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    if digits.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(digits, radix).ok()
}
