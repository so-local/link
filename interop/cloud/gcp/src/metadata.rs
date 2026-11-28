//! This machine's metadata server: what a Compute Engine machine says of
//! itself (its project, its zone and its attached service account) to
//! programs on it, at a fixed address with no credential. `lnk-gcp
//! connect --machine` reads it through [`Metadata`], a seam a test fakes.

use std::time::Duration;

use anyhow::Context;
use serde::Deserialize;

/// Where the metadata server is and what is read from it
/// (`metadata.toml`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    root: String,
    header: Header,
    /// The setting naming where an account's credentials come from.
    pub host_variable: String,
    /// Its value: the metadata server's name.
    pub host: String,
    pub email: String,
    pub project: String,
    pub zone: String,
    connect_seconds: u64,
    request_seconds: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    name: String,
    value: String,
}

/// The metadata server's settings, read once.
pub(crate) fn settings() -> &'static Settings {
    static READ: std::sync::LazyLock<Settings> = std::sync::LazyLock::new(|| {
        let settings: Settings =
            toml::from_str(include_str!("metadata.toml")).expect("checked metadata.toml");
        assert!(settings.connect_seconds > 0 && settings.request_seconds > 0);
        settings
    });
    &READ
}

/// What one read of the metadata server found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reading {
    /// The value, as the server said it.
    Found(String),
    /// The server answered that it has no such value (404).
    Missing,
    /// No metadata server answered: this isn't a Compute Engine machine.
    NoServer,
}

/// A metadata server, asked one value at a time by its path below the
/// server's root (`instance/zone`).
pub(crate) trait Metadata {
    fn read(&self, path: &str) -> anyhow::Result<Reading>;
}

/// This machine's own metadata server, over HTTP.
pub(crate) struct Server {
    client: link_fetch::blocking::Client,
}

impl Server {
    pub fn new() -> anyhow::Result<Self> {
        let settings = settings();
        let client = link_fetch::blocking::Client::builder()
            .metadata_server()
            .outside()
            .connect_timeout(Duration::from_secs(settings.connect_seconds))
            .timeout(Duration::from_secs(settings.request_seconds))
            .build()?;
        Ok(Self { client })
    }
}

impl Metadata for Server {
    fn read(&self, path: &str) -> anyhow::Result<Reading> {
        let settings = settings();
        let url = format!("{}{path}", settings.root);
        match self
            .client
            .get(&url)
            .header(&settings.header.name, &settings.header.value)
            .text()
        {
            Ok(value) => Ok(Reading::Found(value.trim().to_string())),
            Err(err) if err.status() == Some(404) => Ok(Reading::Missing),
            Err(link_fetch::Error::Unreachable { .. }) => Ok(Reading::NoServer),
            Err(err) => {
                Err(err).with_context(|| format!("reading {path} from the metadata server"))
            }
        }
    }
}
