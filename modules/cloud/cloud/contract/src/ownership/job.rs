//! A disposable VM's credential-free identity intent and observed IAM state.
//! The shared project role is a prerequisite, never owned by the launch.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Words {
    journal: String,
    identity_prefix: String,
    description_prefix: String,
    role: String,
    permission: String,
}

static WORDS: LazyLock<Words> =
    LazyLock::new(|| toml::from_str(include_str!("job.toml")).expect("job identity constants"));

pub fn role_id() -> &'static str {
    &WORDS.role
}

pub fn permission() -> &'static str {
    &WORDS.permission
}

pub fn role_name(project: &str) -> String {
    format!("projects/{project}/roles/{}", role_id())
}

pub fn account_id(machine_id: &str) -> String {
    format!("{}{machine_id}", WORDS.identity_prefix)
}

fn machine_name(machine_id: &str) -> String {
    crate::machine_name(&crate::Spec {
        id: Some(machine_id.into()),
        ..Default::default()
    })
}

fn machine_description(place: &str, machine_id: &str) -> String {
    format!(
        "{}{place}/{}",
        WORDS.description_prefix,
        machine_name(machine_id)
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceAccount {
    pub unique_id: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub name: String,
    pub etag: String,
    pub description: String,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub member: String,
    pub role: String,
    pub title: String,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobIdentity {
    pub version: u32,
    pub account: String,
    pub project: String,
    pub project_number: String,
    pub place: String,
    pub machine_id: String,
    pub nonce: String,
    pub email: String,
    pub role: Role,
    pub binding: Binding,
    pub baseline_account: Option<ServiceAccount>,
    pub binding_already_present: bool,
}

fn nonce(value: &str) -> Result<()> {
    ensure!(
        value.len() == 32
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "job identity nonce is not 128-bit lowercase hex"
    );
    Ok(())
}

fn version(value: u32) -> Result<()> {
    ensure!(value == 1, "unsupported job identity journal version");
    Ok(())
}

fn unique_id(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 32 && value.bytes().all(|b| b.is_ascii_digit()),
        "job service account has no immutable numeric uniqueId"
    );
    Ok(())
}

impl JobIdentity {
    pub fn description(&self) -> String {
        format!(
            "{}; journal {}",
            machine_description(&self.place, &self.machine_id),
            self.nonce
        )
    }

    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        nonce(&self.nonce)?;
        link_machine::machine::check_id(&self.machine_id)?;
        unique_id(&self.project_number)?;
        ensure!(
            !self.account.is_empty() && !self.account.contains(char::is_control),
            "job identity has no connected account"
        );
        ensure!(
            [&self.project, &self.place].iter().all(|s| {
                !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }),
            "job identity has an invalid project or place"
        );
        let identity = account_id(&self.machine_id);
        ensure!(
            self.email == format!("{identity}@{}.iam.gserviceaccount.com", self.project),
            "job identity email does not name its machine and project"
        );
        ensure!(
            self.role.name == role_name(&self.project)
                && !self.role.etag.is_empty()
                && self.role.permissions == [permission()],
            "the preprovisioned job role must grant only compute.instances.delete"
        );
        let resource = format!(
            "projects/{}/zones/{}/instances/{}",
            self.project,
            self.place,
            machine_name(&self.machine_id)
        );
        ensure!(
            self.binding
                == Binding {
                    member: format!("serviceAccount:{}", self.email),
                    role: self.role.name.clone(),
                    title: identity,
                    expression: format!("resource.name == '{resource}'"),
                },
            "job binding does not grant only this machine's deletion"
        );
        if let Some(account) = &self.baseline_account {
            unique_id(&account.unique_id)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "event",
    content = "value",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum JobEvent {
    Intent(Box<JobIdentity>),
    Account {
        version: u32,
        nonce: String,
        account: ServiceAccount,
    },
    Binding {
        version: u32,
        nonce: String,
    },
}

link_files::specification!(JobEvent, include_str!("job/event.schema.toml"));

impl JobEvent {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Intent(intent) => intent.validate(),
            Self::Account {
                version: v,
                nonce: n,
                account,
            } => {
                version(*v)?;
                nonce(n)?;
                unique_id(&account.unique_id)?;
                ensure!(
                    !account.description.is_empty(),
                    "observed identity has no creation marker"
                );
                Ok(())
            }
            Self::Binding {
                version: v,
                nonce: n,
            } => {
                version(*v)?;
                nonce(n)
            }
        }
    }
}

pub fn path(launch_journal: &Path) -> PathBuf {
    launch_journal.with_file_name(&WORDS.journal)
}

pub fn journal() -> Option<PathBuf> {
    std::env::var_os(super::JOURNAL_ENV)
        .filter(|p| !p.is_empty())
        .map(|p| path(Path::new(&p)))
}

/// One line of `job.jsonl`, without its newline, its schema advanced: a
/// line a newer release wrote is refused, naming the upgrade.
pub fn parse(line: &[u8]) -> Result<JobEvent> {
    link_files::saved::parse_line(line)
}

/// A complete record, its line naming its schema, is synced before the
/// next provider mutation is allowed.
pub fn append(path: &Path, event: &JobEvent) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    event.validate()?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    file.lock()?;
    file.write_all(&link_files::saved::line(event)?)?;
    file.sync_all()?;
    std::fs::File::open(path.parent().unwrap_or_else(|| Path::new(".")))?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn intent() -> JobIdentity {
        let project = "my-project";
        let place = "us-central1-a";
        let machine_id = "k3v9q2x7m1ab";
        let identity = account_id(machine_id);
        let email = format!("{identity}@{project}.iam.gserviceaccount.com");
        JobIdentity {
            version: 1,
            account: "qa-account".into(),
            project: project.into(),
            project_number: "100200300400".into(),
            place: place.into(),
            machine_id: machine_id.into(),
            nonce: "0123456789abcdef0123456789abcdef".into(),
            email: email.clone(),
            role: Role {
                name: role_name(project),
                etag: "immutable-role-snapshot".into(),
                description: "operator-provisioned shared role".into(),
                permissions: vec![permission().into()],
            },
            binding: Binding {
                member: format!("serviceAccount:{email}"),
                role: role_name(project),
                title: identity,
                expression: format!(
                    "resource.name == 'projects/{project}/zones/{place}/instances/{}'",
                    machine_name(machine_id)
                ),
            },
            baseline_account: None,
            binding_already_present: false,
        }
    }

    #[test]
    fn the_synced_intent_and_immutable_observation_survive_a_lost_response() {
        let directory = link_test_kit::TempDir::new("job-identity-journal");
        let path = directory.join("job.jsonl");
        let intent = intent();
        append(&path, &JobEvent::Intent(Box::new(intent.clone()))).unwrap();
        let account = ServiceAccount {
            unique_id: "123456789012345678901".into(),
            description: intent.description(),
        };
        append(
            &path,
            &JobEvent::Account {
                version: 1,
                nonce: intent.nonce.clone(),
                account: account.clone(),
            },
        )
        .unwrap();
        let records = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| {
                assert!(line.ends_with(r#""schema":1}"#), "{line}");
                parse(line.as_bytes()).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(records[0], JobEvent::Intent(Box::new(intent)));
        assert!(matches!(&records[1], JobEvent::Account { account: got, .. } if got == &account));
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn foreign_scope_or_an_unwritable_intent_refuses_before_any_record() {
        let directory = link_test_kit::TempDir::new("job-identity-refusal");
        let path = directory.join("journal");
        for change in ["member", "resource", "permissions", "nonce"] {
            let mut intent = intent();
            match change {
                "member" => intent.binding.member = "serviceAccount:someone-else".into(),
                "resource" => intent.binding.expression = "true".into(),
                "permissions" => intent
                    .role
                    .permissions
                    .push("compute.instances.start".into()),
                _ => intent.nonce = "predictable".into(),
            }
            assert!(append(&path, &JobEvent::Intent(Box::new(intent))).is_err());
            assert!(!path.exists());
        }
        assert!(
            append(
                &path.join("absent/journal"),
                &JobEvent::Intent(Box::new(intent()))
            )
            .is_err()
        );
    }

    #[test]
    fn old_capability_does_not_claim_job_identity_support() {
        let old: super::super::Schema = serde_json::from_str("{\"version\":1}").unwrap();
        assert!(!old.job_identities);
        let current: super::super::Schema =
            serde_json::from_slice(&super::super::Schema::with_job_identities().bytes()).unwrap();
        assert!(current.job_identities);
    }

    /// A line a newer release wrote is refused, naming the upgrade.
    #[test]
    fn a_newer_line_is_refused() {
        let line = String::from_utf8(
            link_files::saved::line(&JobEvent::Binding {
                version: 1,
                nonce: "n".into(),
            })
            .unwrap(),
        )
        .unwrap()
        .replace(r#""schema":1}"#, r#""schema":2}"#);
        let refused = format!("{:#}", parse(line.trim_end().as_bytes()).unwrap_err());
        assert!(refused.contains("lnk upgrade"), "{refused}");
    }
}
