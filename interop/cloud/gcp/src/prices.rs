//! What Google charges in a region, from its Cloud Billing Catalog API
//! (`services/<Compute Engine>/skus`, reached with the account's own
//! `gcloud` sign-in, as the API has no `gcloud` command): each machine
//! type's on-demand price an hour, from its family's core and memory
//! rates, a GB-month of a balanced and an SSD persistent disk, and an
//! external address an hour. Read once a while
//! ([`link_cloud_contract::prices::cached`]); a machine type whose
//! family's rates aren't there, or with GPUs, gives none, never a guess.

use std::collections::BTreeMap;

use link_cloud_contract::MachineSize;
use link_cloud_contract::prices::{MONTH_HOURS, Prices, Rates};
use serde_json::Value;

use crate::cli::Gcp;

/// Each family Link prices, and its SKUs' descriptions for a core and a
/// GB of memory an hour, on demand.
const FAMILIES: &[(&str, &str, &str)] = &[
    ("e2", "E2 Instance Core", "E2 Instance Ram"),
    ("n2", "N2 Instance Core", "N2 Instance Ram"),
    ("n2d", "N2D AMD Instance Core", "N2D AMD Instance Ram"),
    (
        "n1",
        "N1 Predefined Instance Core",
        "N1 Predefined Instance Ram",
    ),
    ("c3", "C3 Instance Core", "C3 Instance Ram"),
    ("t2d", "T2D AMD Instance Core", "T2D AMD Instance Ram"),
];

/// The share of a core a shared-core machine type is billed for.
const SHARED: &[(&str, f64)] = &[("e2-micro", 0.25), ("e2-small", 0.5), ("e2-medium", 1.0)];

impl Gcp {
    /// The zone's region's prices for `sizes`, kept a while.
    pub fn prices(&self, sizes: &[MachineSize]) -> anyhow::Result<Prices> {
        let region = region(&self.zone);
        link_cloud_contract::prices::cached("gcp", region, || {
            Ok(prices(&self.skus()?, region, sizes))
        })
    }

    /// Every Compute Engine SKU, in dollars.
    fn skus(&self) -> anyhow::Result<Vec<Value>> {
        let token = self.cli.run(&["auth", "print-access-token"])?;
        let token = link_secretive::Secret::new(String::from_utf8_lossy(&token).trim().to_string());
        link_gcp::price_catalog::read(&token)
    }
}

/// Zone `zone`'s region: `us-central1` of `us-central1-a`.
pub fn region(zone: &str) -> &str {
    zone.rsplit_once('-').map_or(zone, |(region, _)| region)
}

/// A SKU's on-demand price a unit in `region`, when it has one: its first
/// tier that's charged.
fn rate(sku: &Value, region: &str) -> Option<(String, f64)> {
    let here = sku["serviceRegions"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|r| r == region || r == "global");
    if !here || sku["category"]["usageType"] != "OnDemand" {
        return None;
    }
    let expression = &sku["pricingInfo"].as_array()?.first()?["pricingExpression"];
    let unit = expression["usageUnit"].as_str()?.to_string();
    expression["tieredRates"]
        .as_array()?
        .iter()
        .filter_map(|tier| {
            let price = &tier["unitPrice"];
            let units = price["units"]
                .as_str()
                .and_then(|u| u.parse::<f64>().ok())?;
            let nanos = price["nanos"].as_f64().unwrap_or(0.0);
            Some(units + nanos / 1e9)
        })
        .find(|usd| *usd > 0.0)
        .map(|usd| (unit, usd))
}

/// The rate of the SKU whose description starts with `description` (and
/// is no spot, preemptible, custom or committed one), in `unit`.
fn find(skus: &[Value], region: &str, description: &str, unit: &str) -> Option<f64> {
    skus.iter()
        .filter(|sku| {
            let words = sku["description"].as_str().unwrap_or_default();
            words.starts_with(description)
                && ![
                    "Spot",
                    "Preemptible",
                    "Custom",
                    "Commitment",
                    "Sole Tenancy",
                ]
                .iter()
                .any(|w| words.contains(w))
        })
        .filter_map(|sku| rate(sku, region))
        .find(|(u, _)| u == unit)
        .map(|(_, usd)| usd)
}

/// `region`'s prices from the catalog's `skus`: each of `sizes` without
/// GPUs from its family's core and memory rates; the disks' and an
/// address's rates.
pub fn prices(skus: &[Value], region: &str, sizes: &[MachineSize]) -> Prices {
    let mut priced = BTreeMap::new();
    for (family, core, ram) in FAMILIES {
        let (Some(core), Some(ram)) = (
            find(skus, region, core, "h"),
            find(skus, region, ram, "GiBy.h"),
        ) else {
            continue;
        };
        for size in sizes.iter().filter(|s| s.gpus == 0) {
            if size.size.split('-').next() != Some(family) {
                continue;
            }
            let cores = SHARED
                .iter()
                .find(|(name, _)| *name == size.size)
                .map_or(f64::from(size.cpus()), |(_, share)| *share);
            let gb = size.memory_mb as f64 / 1024.0;
            priced.insert(size.size.clone(), cores * core + gb * ram);
        }
    }
    let disk = |description: &str| {
        find(skus, region, description, "GiBy.mo").map(|month| month / MONTH_HOURS)
    };
    Prices {
        sizes: priced,
        rates: Rates {
            balanced_disk: disk("Balanced PD Capacity"),
            fast_disk: disk("SSD backed PD Capacity"),
            address: find(skus, region, "External IP Charge on a Standard VM", "h"),
            egress: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A SKU as the catalog lists it.
    fn sku(description: &str, regions: &[&str], unit: &str, units: &str, nanos: u64) -> Value {
        serde_json::json!({
            "description": description,
            "category": {"serviceDisplayName": "Compute Engine", "usageType": "OnDemand"},
            "serviceRegions": regions,
            "pricingInfo": [{"pricingExpression": {"usageUnit": unit, "tieredRates": [
                {"startUsageAmount": 0, "unitPrice": {"currencyCode": "USD", "units": units, "nanos": nanos}}
            ]}}]
        })
    }

    fn size(name: &str, cpus: u32, memory_mb: u64) -> MachineSize {
        MachineSize {
            size: name.into(),
            cpus: cpus.into(),
            memory_mb,
            gpus: 0,
            gpu_model: None,
            arch: "x86_64".into(),
            price: None,
        }
    }

    /// Recorded SKUs give a size's price from its family's rates, a
    /// shared-core one's from its share, the disks' and an address's; a
    /// size whose family has no rates there, or another region's, none.
    #[test]
    fn the_catalog_is_read_into_prices() {
        let skus = [
            sku(
                "E2 Instance Core running in Americas",
                &["us-central1"],
                "h",
                "0",
                21_811_590,
            ),
            sku(
                "E2 Instance Ram running in Americas",
                &["us-central1"],
                "GiBy.h",
                "0",
                2_923_530,
            ),
            sku(
                "Spot Preemptible E2 Instance Core running in Americas",
                &["us-central1"],
                "h",
                "0",
                6_543_000,
            ),
            sku(
                "N2 Instance Core running in Paris",
                &["europe-west9"],
                "h",
                "0",
                37_000_000,
            ),
            sku(
                "N2 Instance Ram running in Paris",
                &["europe-west9"],
                "GiBy.h",
                "0",
                5_000_000,
            ),
            sku(
                "Balanced PD Capacity in Iowa",
                &["us-central1"],
                "GiBy.mo",
                "0",
                100_000_000,
            ),
            sku(
                "External IP Charge on a Standard VM",
                &["global"],
                "h",
                "0",
                4_000_000,
            ),
        ];
        let sizes = [
            size("e2-standard-2", 2, 8192),
            size("e2-micro", 2, 1024),
            size("n2-standard-2", 2, 8192),
        ];
        let prices = prices(&skus, "us-central1", &sizes);
        let e2 = prices.sizes["e2-standard-2"];
        assert!(
            (e2 - (2.0 * 0.02181159 + 8.0 * 0.00292353)).abs() < 1e-9,
            "{e2}"
        );
        let micro = prices.sizes["e2-micro"];
        assert!(
            (micro - (0.25 * 0.02181159 + 0.00292353)).abs() < 1e-9,
            "{micro}"
        );
        assert!(!prices.sizes.contains_key("n2-standard-2"));
        assert!((prices.rates.balanced_disk.unwrap() - 0.1 / 730.0).abs() < 1e-12);
        assert_eq!(prices.rates.fast_disk, None);
        assert_eq!(prices.rates.address, Some(0.004));
        assert_eq!(region("us-central1-a"), "us-central1");
    }
}
