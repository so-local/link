//! What a cloud charges, as its adapter reads it from the cloud's own
//! price API: each machine type's hourly price (`MachineSize::price`) and
//! a region's rates for the rest of a box ([`Rates`]), cached in the
//! adapter's settings folder for as long as its `prices` setting says
//! (`~/.lnk/<kind>/settings.toml`, a day by default).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::want::DiskType;

/// How long a price is kept when the adapter's settings say nothing.
pub const KEPT: Duration = Duration::from_secs(86_400);

/// A region's rates for what a box has beside its machine type, each
/// none when the cloud gives none: generated from the contract's
/// declarations.
pub use crate::declared::Rates;

impl Rates {
    /// A GB of a disk of `kind`, an hour.
    pub fn disk(&self, kind: DiskType) -> Option<f64> {
        match kind {
            DiskType::Balanced => self.balanced_disk,
            DiskType::Fast => self.fast_disk,
        }
    }

    /// The price of a box of a machine type at `machine` an hour, with a
    /// `disk_gb` disk of `kind` and a public address.
    pub fn price(&self, machine: Option<f64>, kind: DiskType, disk_gb: u64) -> Price {
        Price {
            machine,
            disk: self.disk(kind).map(|rate| rate * disk_gb as f64),
            address: self.address,
            egress: self.egress,
        }
    }
}

/// A month's hours, as clouds price a GB-month: 730.
pub const MONTH_HOURS: f64 = 730.0;

/// A region's prices, as an adapter read them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prices {
    /// Each machine type's, an hour, by its name.
    #[serde(default)]
    pub sizes: std::collections::BTreeMap<String, f64>,
    #[serde(default)]
    pub rates: Rates,
}

// Kept as `prices/<place>.json` in its adapter's folder, naming its
// schema.
link_files::specification!(Prices, include_str!("prices/kept.schema.toml"));

/// Each cloud's prices, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Prices>()
}

/// The adapter's settings this reads.
#[derive(Debug, Default, Deserialize)]
struct Settings {
    #[serde(default)]
    prices: Option<String>,
}

/// How long adapter `kind`'s prices are kept: its settings' `prices`
/// (`"1d"`, `"12h"`, `"30m"`), else [`KEPT`].
pub fn kept(path: &Path) -> anyhow::Result<Duration> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(KEPT);
    };
    let settings: Settings =
        toml::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let Some(prices) = settings.prices else {
        return Ok(KEPT);
    };
    let (number, unit) = prices.split_at(prices.len().saturating_sub(1));
    let unit = match unit {
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => 0,
    };
    let secs = number.parse::<u64>().ok().and_then(|n| n.checked_mul(unit));
    secs.filter(|s| *s > 0)
        .map(Duration::from_secs)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{}: prices = {prices:?}: say how long as 30m, 12h or 1d",
                path.display()
            )
        })
}

/// Region `place`'s prices for adapter `kind`: those kept in `dir` while
/// they're younger than its settings' `prices`, else `read`'s, then kept.
/// `read` failing gives its error, and nothing is kept.
pub fn cached(
    kind: &str,
    place: &str,
    read: impl FnOnce() -> anyhow::Result<Prices>,
) -> anyhow::Result<Prices> {
    let dir = link_files::layout::Hidden::Prices.try_path()?.join(kind);
    cached_in(&dir, place, read)
}

/// [`cached`], in folder `dir` (the adapter's settings folder).
pub fn cached_in(
    dir: &Path,
    place: &str,
    read: impl FnOnce() -> anyhow::Result<Prices>,
) -> anyhow::Result<Prices> {
    anyhow::ensure!(
        !place.is_empty()
            && place.len() <= 64
            && place
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "{place:?} isn't a region or zone"
    );
    let kept = kept(&dir.join("settings.toml"))?;
    let path = file(dir, place);
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age < kept);
    if fresh
        && let Some(prices) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| link_files::saved::parse_json(&bytes).ok())
    {
        return Ok(prices);
    }
    let prices = read()?;
    if let Some(parent) = path.parent() {
        link_files::create_private_dir(parent)?;
    }
    link_files::write_private(&path, &link_files::saved::print_json(&prices)?)?;
    Ok(prices)
}

fn file(dir: &Path, place: &str) -> PathBuf {
    dir.join("prices").join(format!("{place}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prices read once are read from the cache while they're younger
    /// than the setting says; past it, or with a setting that's no
    /// duration, not.
    #[test]
    fn prices_are_read_once_a_while() {
        let dir = link_test_kit::TempDir::new("provider-prices");
        let read = || {
            Ok(Prices {
                sizes: [("t3.nano".to_string(), 0.0052)].into(),
                rates: Rates::default(),
            })
        };
        let first = cached_in(&dir, "us-east-1", read).unwrap();
        let again =
            cached_in(&dir, "us-east-1", || anyhow::bail!("asked the cloud again")).unwrap();
        assert_eq!(first, again);
        // Kept for a minute, and past that: read again.
        std::fs::write(dir.join("settings.toml"), "prices = \"1m\"\n").unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(120);
        std::fs::File::options()
            .write(true)
            .open(file(&dir, "us-east-1"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        let err = cached_in(&dir, "us-east-1", || anyhow::bail!("asked")).unwrap_err();
        assert!(format!("{err:#}").contains("asked"));
        std::fs::write(dir.join("settings.toml"), "prices = \"soon\"\n").unwrap();
        assert!(kept(&dir.join("settings.toml")).is_err());
        assert_eq!(kept(&dir.join("none.toml")).unwrap(), KEPT);
    }

    /// A box's price: its machine type's, its disk at its kind's rate by
    /// its size, its address's.
    #[test]
    fn a_boxs_price_is_its_rates_by_its_parts() {
        let rates = Rates {
            balanced_disk: Some(0.08 / MONTH_HOURS),
            fast_disk: None,
            address: Some(0.005),
            egress: Some(0.09),
        };
        let price = rates.price(Some(0.0052), DiskType::Balanced, 20);
        assert!((price.disk.unwrap() - 20.0 * 0.08 / 730.0).abs() < 1e-12);
        assert_eq!(price.address, Some(0.005));
        assert_eq!(rates.price(Some(0.1), DiskType::Fast, 20).disk, None);
    }
}

/// What a box costs while it's there, in dollars, each part only from a
/// rate its cloud gives: none is none, never a guess.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Price {
    /// Its machine type, an hour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<f64>,
    /// Its disk, as big as it is, an hour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk: Option<f64>,
    /// Its public address, an hour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<f64>,
    /// What it sends out to the internet, a GB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress: Option<f64>,
}

impl Price {
    /// Its machine, disk and address an hour, and whether a part of them
    /// has no price; none without its machine's.
    pub fn hourly(&self) -> Option<(f64, bool)> {
        let machine = self.machine?;
        let partial = self.disk.is_none() || self.address.is_none();
        Some((
            machine + self.disk.unwrap_or(0.0) + self.address.unwrap_or(0.0),
            partial,
        ))
    }
}

#[cfg(test)]
mod price_tests {
    use super::Price;

    /// A box's price is its parts an hour; one with a part unknown is
    /// partial, and one without its machine's is none.
    #[test]
    fn a_price_is_its_parts() {
        let typed: Price =
            serde_json::from_str(r#"{"machine":0.1,"disk":0.01,"address":0.005}"#).unwrap();
        let (hourly, partial) = typed.hourly().unwrap();
        assert!((hourly - 0.115).abs() < 1e-9 && !partial);
        let machine: Price = serde_json::from_str(r#"{"machine":0.12}"#).unwrap();
        assert_eq!(machine.hourly(), Some((0.12, true)));
        assert!(serde_json::from_str::<Price>("0.12").is_err());
        assert_eq!(Price::default().hourly(), None);
    }
}
