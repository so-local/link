//! Sending measures to an OTLP endpoint you name (`lnk measure export set`):
//! OTLP/HTTP, as JSON, to `<url>/v1/metrics`, so an OpenTelemetry
//! collector, Grafana, Honeycomb or your own reads them unchanged.

use std::sync::mpsc;
use std::time::Duration;

use anyhow::bail;
use link_secretive::Duplicate;

use crate::settings::Export;

/// How long a request has to be answered: a slow endpoint never holds
/// the watcher up for long.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The most looks waiting to be sent: past it, a look is dropped
/// rather than the watcher held up.
const WAITING: usize = 6;

/// Send an explicitly selected retained profile once, waiting for the
/// endpoint's acknowledgement instead of dropping it on a full queue.
pub fn once(export: &Export, request: &serde_json::Value) -> anyhow::Result<()> {
    let client = link_fetch::blocking::Client::builder()
        .timeout(TIMEOUT)
        .outside()
        .build()?;
    post(&client, export, request)
}

/// Sends to an endpoint on a thread of its own, one request at a time,
/// so a slow one never holds a look up.
pub struct Sender {
    queue: mpsc::SyncSender<(Export, serde_json::Value)>,
}

impl Sender {
    pub fn new() -> anyhow::Result<Self> {
        // An endpoint of the user's own: plain http:// only to this
        // machine, as its `SendUrl` is.
        let client = link_fetch::blocking::Client::builder()
            .timeout(TIMEOUT)
            .outside()
            .build()?;
        let (queue, waiting) = mpsc::sync_channel::<(Export, serde_json::Value)>(WAITING);
        std::thread::spawn(move || {
            for (export, request) in waiting {
                if let Err(err) = post(&client, &export, &request) {
                    crate::problem("sending a look on", &err);
                }
            }
        });
        Ok(Self { queue })
    }

    /// Posts `request` (OTLP JSON) to `export`'s endpoint, after those
    /// waiting; dropped when too many are.
    pub fn send(&self, export: &Export, request: serde_json::Value) -> anyhow::Result<()> {
        match self.queue.try_send((export.duplicate(), request)) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => {
                bail!(
                    "{} is slow to answer: a look wasn't sent",
                    export.url.host()
                )
            }
            Err(mpsc::TrySendError::Disconnected(_)) => bail!("the sender stopped"),
        }
    }
}

fn post(
    client: &link_fetch::blocking::Client,
    export: &Export,
    request: &serde_json::Value,
) -> anyhow::Result<()> {
    let url = link_measure_contract::client::metrics_url(export.url.as_str());
    let mut post = client.post(&url).json(request);
    for (name, value) in &export.headers {
        post = post.secret_header(name, value);
    }
    let host = export.url.host();
    match post.send() {
        Ok(()) => Ok(()),
        Err(err) => match err.status() {
            Some(status) => bail!("{host} answered {status}"),
            None => Err(anyhow::Error::from(err).context(format!("sending to {host}"))),
        },
    }
}
