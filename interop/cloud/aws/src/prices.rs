//! What AWS charges in a region, from its Price List Query API (`aws
//! pricing get-products`, the same CLI): each instance type's on-demand
//! Linux price an hour, a GB-month of gp3 (balanced) and io2 (fast)
//! storage, a public IPv4 address an hour and a GB out to the internet.
//! Read once a while ([`link_cloud_contract::prices::cached`]); an
//! answer without a price gives none, never a guess.

use std::collections::BTreeMap;

use link_cloud_contract::prices::{MONTH_HOURS, Prices, Rates};
use serde_json::Value;

use crate::cli::Aws;

/// The Price List Query API's endpoint: it answers for every region from
/// here.
const PRICING_REGION: &str = "us-east-1";

impl Aws {
    /// The region's prices, kept a while.
    pub fn prices(&self) -> anyhow::Result<Prices> {
        link_cloud_contract::prices::cached("aws", &self.region, || {
            Ok(Prices {
                sizes: instances(&self.products(&[
                    ("productFamily", "Compute Instance"),
                    ("operatingSystem", "Linux"),
                    ("tenancy", "Shared"),
                    ("preInstalledSw", "NA"),
                    ("capacitystatus", "Used"),
                ])?),
                rates: Rates {
                    balanced_disk: storage(&self.products(&storage_filter("gp3"))?),
                    fast_disk: storage(&self.products(&storage_filter("io2"))?),
                    address: address(&self.products(&[("productFamily", "IP Address")])?),
                    egress: egress(&self.products(&[("transferType", "AWS Outbound")])?),
                },
            })
        })
    }

    /// The products the filters match in this region, each its JSON.
    fn products(&self, filters: &[(&str, &str)]) -> anyhow::Result<Vec<Value>> {
        let mut args = vec![
            "pricing".to_string(),
            "get-products".into(),
            "--region".into(),
            PRICING_REGION.into(),
            "--service-code".into(),
            "AmazonEC2".into(),
            "--filters".into(),
        ];
        let region = [("regionCode", self.region.as_str())];
        for (field, value) in filters.iter().chain(&region) {
            args.push(format!("Type=TERM_MATCH,Field={field},Value={value}"));
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        Ok(products(&self.cli.json(&args)?))
    }
}

fn storage_filter(volume: &str) -> [(&'static str, &str); 2] {
    [("productFamily", "Storage"), ("volumeApiName", volume)]
}

/// `get-products`' answer's products: its `PriceList`, each a string of
/// JSON.
pub fn products(answer: &Value) -> Vec<Value> {
    answer["PriceList"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| match p {
            Value::String(text) => serde_json::from_str(text).ok(),
            other => Some(other.clone()),
        })
        .collect()
}

/// A product's on-demand prices in dollars, each with its unit and the
/// start of its range.
fn on_demand(product: &Value) -> Vec<(String, f64, f64)> {
    let mut prices = Vec::new();
    for term in product["terms"]["OnDemand"]
        .as_object()
        .into_iter()
        .flatten()
    {
        for dimension in term.1["priceDimensions"].as_object().into_iter().flatten() {
            let dimension = dimension.1;
            let usd = dimension["pricePerUnit"]["USD"]
                .as_str()
                .and_then(|p| p.parse::<f64>().ok());
            let begin = dimension["beginRange"]
                .as_str()
                .and_then(|b| b.parse::<f64>().ok())
                .unwrap_or(0.0);
            if let Some(usd) = usd.filter(|u| u.is_finite() && *u >= 0.0) {
                let unit = dimension["unit"].as_str().unwrap_or_default().to_string();
                prices.push((unit, usd, begin));
            }
        }
    }
    prices
}

/// Each instance type's price an hour, by its name.
pub fn instances(products: &[Value]) -> BTreeMap<String, f64> {
    let mut sizes = BTreeMap::new();
    for product in products {
        let Some(size) = product["product"]["attributes"]["instanceType"].as_str() else {
            continue;
        };
        let hourly = on_demand(product)
            .into_iter()
            .find(|(unit, usd, _)| unit == "Hrs" && *usd > 0.0);
        if let Some((_, usd, _)) = hourly {
            sizes.insert(size.to_string(), usd);
        }
    }
    sizes
}

/// A GB of the volume an hour, from its price a GB-month.
pub fn storage(products: &[Value]) -> Option<f64> {
    products
        .iter()
        .flat_map(on_demand)
        .find(|(unit, _, _)| unit == "GB-Mo")
        .map(|(_, usd, _)| usd / MONTH_HOURS)
}

/// An in-use public IPv4 address an hour.
pub fn address(products: &[Value]) -> Option<f64> {
    products
        .iter()
        .filter(|p| {
            p["product"]["attributes"]["usagetype"]
                .as_str()
                .is_some_and(|u| u.ends_with("PublicIPv4:InUseAddress"))
        })
        .flat_map(on_demand)
        .find(|(unit, _, _)| unit == "Hrs")
        .map(|(_, usd, _)| usd)
}

/// A GB out to the internet: the first tier that's charged.
pub fn egress(products: &[Value]) -> Option<f64> {
    let mut tiers: Vec<(f64, f64)> = products
        .iter()
        .filter(|p| p["product"]["attributes"]["toLocation"] == "External")
        .flat_map(on_demand)
        .filter(|(unit, usd, _)| unit == "GB" && *usd > 0.0)
        .map(|(_, usd, begin)| (begin, usd))
        .collect();
    tiers.sort_by(|a, b| a.0.total_cmp(&b.0));
    tiers.first().map(|(_, usd)| *usd)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A product as `get-products` lists it: a string of JSON.
    fn listed(attributes: Value, unit: &str, usd: &str, begin: &str) -> Value {
        let product = serde_json::json!({
            "product": {"attributes": attributes, "sku": "SKU1"},
            "terms": {"OnDemand": {"SKU1.TERM": {"priceDimensions": {"SKU1.TERM.DIM": {
                "unit": unit, "beginRange": begin, "endRange": "Inf",
                "pricePerUnit": {"USD": usd}
            }}}}}
        });
        Value::String(product.to_string())
    }

    /// Recorded answers parsed into a size's, a disk's, an address's and
    /// egress's prices; an answer without the size gives none of it.
    #[test]
    fn answers_are_read_into_prices() {
        let answer = serde_json::json!({"PriceList": [
            listed(serde_json::json!({"instanceType": "t3.nano"}), "Hrs", "0.0052000000", "0"),
            listed(serde_json::json!({"instanceType": "t3.small"}), "Hrs", "0.0208000000", "0"),
            listed(serde_json::json!({"instanceType": "t3.gone"}), "Hrs", "0.0000000000", "0"),
        ], "FormatVersion": "aws_v1"});
        let sizes = instances(&products(&answer));
        assert_eq!(sizes.get("t3.nano"), Some(&0.0052));
        assert_eq!(sizes.len(), 2, "{sizes:?}");
        assert_eq!(sizes.get("m7i.large"), None);
        let gp3 = products(&serde_json::json!({"PriceList": [
            listed(serde_json::json!({"volumeApiName": "gp3"}), "GB-Mo", "0.0800000000", "0"),
        ]}));
        assert!((storage(&gp3).unwrap() - 0.08 / 730.0).abs() < 1e-12);
        let ips = products(&serde_json::json!({"PriceList": [
            listed(serde_json::json!({"usagetype": "USE1-PublicIPv4:IdleAddress"}), "Hrs", "0.0050000000", "0"),
            listed(serde_json::json!({"usagetype": "USE1-PublicIPv4:InUseAddress"}), "Hrs", "0.0050000000", "0"),
        ]}));
        assert_eq!(address(&ips), Some(0.005));
        let out = products(&serde_json::json!({"PriceList": [
            listed(serde_json::json!({"toLocation": "External"}), "GB", "0.0000000000", "0"),
            listed(serde_json::json!({"toLocation": "External"}), "GB", "0.0500000000", "153600"),
            listed(serde_json::json!({"toLocation": "External"}), "GB", "0.0900000000", "100"),
            listed(serde_json::json!({"toLocation": "US East (Ohio)"}), "GB", "0.0100000000", "0"),
        ]}));
        assert_eq!(egress(&out), Some(0.09));
        assert_eq!(storage(&[]), None);
        assert_eq!(address(&[]), None);
    }
}
