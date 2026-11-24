//! A durable, credential-free intent before a provider allocates a machine.
//! Callers can reconcile a lost launch response by the generated installation ID.

use std::io::Write;
use std::path::Path;

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::Request;

pub mod firewall;
pub mod job;
pub use job::{Binding, JobEvent, JobIdentity, Role, ServiceAccount};

pub const JOURNAL_ENV: &str = "LNK_RESOURCE_JOURNAL";
pub const SCHEMA_ARGUMENT: &str = "resource-journal-schema";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    pub version: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub job_identities: bool,
}

impl Schema {
    /// A provider that journals the identity and conditional IAM grant too.
    pub fn with_job_identities() -> Self {
        Self {
            version: 1,
            job_identities: true,
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a schema version serializes")
    }
}

pub fn schema_bytes() -> Vec<u8> {
    Schema {
        version: 1,
        job_identities: false,
    }
    .bytes()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub version: u32,
    pub kind: String,
    pub account: String,
    pub place: String,
    pub name: String,
    pub machine_id: String,
    pub owner: String,
}

link_files::specification!(Launch, include_str!("ownership/launch.schema.toml"));

/// One line of the resource journal, without its newline, its schema
/// advanced: a line a newer release wrote is refused, naming the upgrade.
pub fn parse_launch(line: &[u8]) -> Result<Launch> {
    link_files::saved::parse_line(line)
}

impl Launch {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported resource intent version");
        crate::check_kind(&self.kind)?;
        link_machine::machine::check_id(&self.machine_id)?;
        ensure!(
            !self.account.is_empty() && !self.place.is_empty() && !self.owner.is_empty(),
            "resource intent requires account, place and owner"
        );
        link_machine::machine::check_name(&self.name)?;
        Ok(())
    }

    fn of(kind: &str, request: &Request) -> Result<Self> {
        let spec = request
            .spec
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("launch has no spec"))?;
        let intent = Launch {
            version: 1,
            kind: kind.into(),
            account: request.account.name.clone(),
            place: request
                .place
                .clone()
                .unwrap_or_else(|| request.account.place.clone()),
            name: spec.name.clone(),
            machine_id: spec.id().to_string(),
            owner: spec.tags.get("lnk-owner").cloned().unwrap_or_default(),
        };
        intent.validate()?;
        ensure!(
            request.account.kind == kind,
            "launch account has another adapter kind"
        );
        ensure!(
            spec.tags.get("lnk-machine") == Some(&intent.machine_id)
                && spec.tags.get("lnk-box") == Some(&intent.name),
            "launch tags do not bind its installation identity"
        );
        Ok(intent)
    }
}

pub(crate) fn around<T>(
    kind: &str,
    request: &Request,
    launch: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let path = std::env::var_os(JOURNAL_ENV).filter(|v| !v.is_empty());
    before(path.as_deref().map(Path::new), kind, request, launch)
}

fn before<T>(
    path: Option<&Path>,
    kind: &str,
    request: &Request,
    launch: impl FnOnce() -> Result<T>,
) -> Result<T> {
    if let Some(path) = path {
        use std::os::unix::fs::OpenOptionsExt;
        let intent = Launch::of(kind, request)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
        file.lock()?;
        file.write_all(&link_files::saved::line(&intent)?)?;
        file.sync_all()?;
    }
    launch()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Request {
        serde_json::from_value(serde_json::json!({
            "account":{"name":"qa","kind":"aws","place":"region-1","env":{"SECRET":"never-journal-this"}},
            "place":"region-2", "spec":{"name":"owned","id":"k3v9q2x7m1ab",
                "tags":{"lnk-owner":"owner-one","lnk-machine":"k3v9q2x7m1ab","lnk-box":"owned"},
                "size":"small","user_data":"also-not-journaled"}
        })).unwrap()
    }

    #[test]
    fn launch_intent_is_durable_before_provider_call_even_when_response_is_lost() {
        let dir = link_test_kit::temp::TempDir::new("launch-intent");
        let path = dir.path().join("intents.jsonl");
        let result: Result<()> = before(Some(&path), "aws", &request(), || {
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(!text.contains("never-journal-this") && !text.contains("also-not-journaled"));
            let intent = parse_launch(text.trim().as_bytes()).unwrap();
            assert_eq!(intent.machine_id, "k3v9q2x7m1ab");
            assert_eq!(intent.place, "region-2");
            anyhow::bail!("provider created it but its response was lost")
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 1);
    }

    #[test]
    fn an_unwritable_intent_or_mismatched_identity_refuses_before_provider_call() {
        let dir = link_test_kit::temp::TempDir::new("launch-intent-refusal");
        let absent = dir.path().join("absent/intents");
        let result: Result<()> = before(Some(&absent), "aws", &request(), || {
            panic!("provider must not run")
        });
        assert!(result.is_err());
        let mut request = request();
        request
            .spec
            .as_mut()
            .unwrap()
            .tags
            .insert("lnk-machine".into(), "foreign".into());
        let result: Result<()> = before(Some(&dir.path().join("journal")), "aws", &request, || {
            panic!("provider must not run")
        });
        assert!(result.is_err());
    }
}
