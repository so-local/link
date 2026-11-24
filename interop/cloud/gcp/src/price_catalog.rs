//! Bounded complete reads of Google's external Compute Engine price catalogue.
use anyhow::{Context, ensure};
use link_fetch::blocking::Client;
use link_secretive::Secret;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    api: String,
    pages: usize,
    page_size: u32,
    page_bytes: u64,
    request_seconds: u64,
}

fn settings() -> &'static Settings {
    static READ: std::sync::LazyLock<Settings> = std::sync::LazyLock::new(|| {
        let settings: Settings =
            toml::from_str(include_str!("price_catalog.toml")).expect("checked price_catalog.toml");
        assert!(
            settings.pages > 0
                && settings.page_size > 0
                && settings.page_bytes > 0
                && settings.request_seconds > 0
        );
        settings
    });
    &READ
}

#[derive(Deserialize)]
struct Page {
    skus: Vec<Value>,
    #[serde(default, rename = "nextPageToken")]
    next: String,
}

pub fn read(token: &Secret) -> anyhow::Result<Vec<Value>> {
    let settings = settings();
    let client = Client::builder()
        .outside()
        .https_only()
        .timeout(std::time::Duration::from_secs(settings.request_seconds))
        .build()?;
    read_with(settings, &client, token)
}

fn read_with(settings: &Settings, client: &Client, token: &Secret) -> anyhow::Result<Vec<Value>> {
    let mut skus = Vec::new();
    let mut page = String::new();
    let mut seen = std::collections::BTreeSet::new();
    let page_size = settings.page_size.to_string();
    for _ in 0..settings.pages {
        let mut query = vec![("currencyCode", "USD"), ("pageSize", page_size.as_str())];
        if !page.is_empty() {
            query.push(("pageToken", &page));
        }
        let answer: Page = client
            .get(&settings.api)
            .bearer(token)
            .query(&query)
            .limit(settings.page_bytes)
            .answer()
            .context("reading Google's price catalog")?;
        skus.extend(answer.skus);
        if answer.next.is_empty() {
            return Ok(skus);
        }
        ensure!(
            seen.insert(answer.next.clone()),
            "the price catalog repeated its page token"
        );
        page = answer.next;
    }
    anyhow::bail!("the price catalog exceeds its configured page budget; no partial quote was kept")
}

#[cfg(test)]
mod tests;
