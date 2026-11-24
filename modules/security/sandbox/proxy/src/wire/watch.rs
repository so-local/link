//! The watch: calls that look like no normal work pause a wire.

use super::*;

/// What a wire watches its calls for: patterns no normal work makes.
/// Loose thresholds, so a busy program isn't paused; a day's log of
/// normal work tightens them.
#[derive(Debug, Default)]
pub struct Watch {
    /// Refused calls within [`PROBE_WINDOW`], at most [`MAX_REFUSED`]:
    /// when, and where to, oldest first.
    pub(super) refused: std::collections::VecDeque<(u64, String)>,
    /// How many of `refused` went to each target.
    pub(super) targets: std::collections::HashMap<String, usize>,
}

/// The most refusals a watch keeps: far past what any pattern needs.
pub(super) const MAX_REFUSED: usize = 1024;

/// Refusals within [`BURST_WINDOW`] that pause a wire.
pub const BURST: usize = 30;
pub const BURST_WINDOW: u64 = 60;
/// Different targets refused within [`PROBE_WINDOW`] that pause a wire:
/// hosts or ports tried in turn.
pub const PROBE: usize = 20;
pub const PROBE_WINDOW: u64 = 600;

impl Watch {
    /// The pattern `entry` completes, if any, as a pause says it.
    pub fn saw(&mut self, entry: &Entry) -> Option<String> {
        if entry.verb == "pause" {
            return None;
        }
        if metadata(&entry.to) {
            return Some(format!(
                "it asked for a cloud's metadata service ({})",
                entry.to
            ));
        }
        if entry.outcome != Outcome::Refused {
            return None;
        }
        self.refused.push_back((entry.time, entry.to.clone()));
        *self.targets.entry(entry.to.clone()).or_default() += 1;
        while let Some((t, _)) = self.refused.front()
            && (entry.time.saturating_sub(*t) > PROBE_WINDOW || self.refused.len() > MAX_REFUSED)
        {
            let Some((_, to)) = self.refused.pop_front() else {
                break;
            };
            if let Some(count) = self.targets.get_mut(&to) {
                *count -= 1;
                if *count == 0 {
                    self.targets.remove(&to);
                }
            }
        }
        // The latest, newest first, up to the one that would pause.
        let recent = self
            .refused
            .iter()
            .rev()
            .take_while(|(t, _)| entry.time.saturating_sub(*t) <= BURST_WINDOW)
            .take(BURST)
            .count();
        if recent >= BURST {
            return Some(format!("{recent} calls were refused within a minute"));
        }
        if self.targets.len() >= PROBE {
            return Some(format!(
                "{} different hosts or ports were refused within 10 minutes",
                self.targets.len()
            ));
        }
        None
    }
}

/// A call refused because it resolved to a cloud's metadata service,
/// however its host was written (`2852039166`, `0xa9fea9fe`, a name
/// pointing there): the wire pauses on it at once.
#[derive(Debug)]
pub struct Metadata(pub String);

impl std::fmt::Display for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "refused: {} is a cloud's metadata service", self.0)
    }
}

impl std::error::Error for Metadata {}

/// Clouds' metadata services' IPv4 addresses: the link-local one AWS,
/// Google Cloud, Azure and most others answer on, and Alibaba Cloud's,
/// in the carrier-grade range.
const METADATA_V4: [[u8; 4]; 2] = [[169, 254, 169, 254], [100, 100, 100, 200]];

/// Clouds' metadata services' IPv6 addresses: AWS's and Google Cloud's,
/// each in the private range `[lan]` may list.
const METADATA_V6: [[u16; 8]; 2] = [
    [0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254],
    [0xfd20, 0xce, 0, 0, 0, 0, 0, 0x254],
];

/// Whether `ip` is a cloud's metadata service's address ([`METADATA_V4`],
/// [`METADATA_V6`], or IPv4's written as IPv6: mapped, compatible, NAT64,
/// 6to4).
pub fn is_metadata(ip: std::net::IpAddr) -> bool {
    let ip = crate::lan::canonical(ip);
    METADATA_V4
        .iter()
        .any(|v4| ip == std::net::IpAddr::from(*v4))
        || METADATA_V6
            .iter()
            .any(|v6| ip == std::net::IpAddr::from(*v6))
}

/// Whether `to` (`host:port`) is a cloud's metadata service: by its
/// address, however it's written (in IPv6, or a URL's number), or by its
/// name.
pub(super) fn metadata(to: &str) -> bool {
    let host = crate::http::host_port(to, Some(0)).map_or_else(|| to.to_string(), |(host, _)| host);
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let ip = host
        .parse::<std::net::IpAddr>()
        .ok()
        .or_else(|| crate::http::url_ipv4(&host).map(std::net::IpAddr::V4));
    if let Some(ip) = ip {
        return is_metadata(ip);
    }
    [
        "169.254.169.254",
        "fd00:ec2::254",
        "metadata.google.internal",
        "metadata",
    ]
    .contains(&host.as_str())
}
