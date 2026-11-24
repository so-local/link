//! Journaled job launches own a nonce-marked account, never the shared role.
use super::*;
use link_cloud_contract::ownership::job::{
    self, Binding, JobEvent, JobIdentity, Role, ServiceAccount,
};
use serde_json::Value;
use std::path::Path;

impl Gcp {
    pub(super) fn journaled_job_identity(
        &self,
        path: &Path,
        spec: &Spec,
    ) -> anyhow::Result<String> {
        link_machine::machine::check_id(spec.id())?;
        // A custom role is the project's: gcloud reads it only with --project.
        let raw = self.cli.json(&["iam", "roles", "describe", job::role_id(), &format!("--project={}", self.project), "--format=json"])
            .map_err(|_| anyhow::anyhow!("the shared disposable-job role must be preprovisioned before a journaled launch"))?;
        ensure!(
            raw.get("deleted").is_none_or(|v| v == false),
            "the shared disposable-job role is deleted"
        );
        let role = Role {
            name: string(&raw, "name")?,
            etag: string(&raw, "etag")?,
            description: raw["description"].as_str().unwrap_or_default().into(),
            permissions: raw["includedPermissions"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("the shared role has no permissions inventory"))?
                .iter()
                .map(|p| {
                    p.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| anyhow::anyhow!("invalid role permission"))
                })
                .collect::<anyhow::Result<_>>()?,
        };
        let project = self
            .cli
            .json(&["projects", "describe", &self.project, "--format=json"])
            .map_err(|_| anyhow::anyhow!("the native project identity is unreadable"))?;
        ensure!(
            project["projectId"].as_str() == Some(&self.project),
            "the native project ID differs from the frozen account"
        );
        let identity = job::account_id(spec.id());
        let email = format!("{identity}@{}.iam.gserviceaccount.com", self.project);
        let intent = JobIdentity {
            version: 1,
            account: self.account.clone(),
            project: self.project.clone(),
            project_number: string(&project, "projectNumber")?,
            place: self.zone.clone(),
            machine_id: spec.id().to_string(),
            nonce: link_files::random_token()?,
            email: email.clone(),
            role,
            binding: Binding {
                member: format!("serviceAccount:{email}"),
                role: job::role_name(&self.project),
                title: identity.clone(),
                expression: format!(
                    "resource.name == 'projects/{}/zones/{}/instances/{}'",
                    self.project,
                    self.zone,
                    link_cloud_contract::machine_name(spec)
                ),
            },
            baseline_account: None,
            binding_already_present: false,
        };
        intent.validate()?;
        // A reusable email or old binding is not creation authority. Journaled
        // launches never borrow it, reap other accounts or modify shared roles.
        match self.cli.json(&[
            "iam",
            "service-accounts",
            "describe",
            &email,
            "--format=json",
        ]) {
            Ok(_) => anyhow::bail!("the disposable identity already exists; it was preserved"),
            Err(e) if absent(&e.to_string(), "iam.service-accounts.describe") => {}
            Err(_) => anyhow::bail!("the disposable identity baseline is unreadable"),
        }
        let policy = self
            .cli
            .json(&["projects", "get-iam-policy", &self.project, "--format=json"])
            .map_err(|_| anyhow::anyhow!("the native IAM baseline is unreadable"))?;
        let bindings = policy["bindings"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("the native IAM baseline has no bindings inventory"))?;
        for binding in bindings {
            for member in binding["members"].as_array().ok_or_else(|| {
                anyhow::anyhow!("the native IAM baseline has no members inventory")
            })? {
                let member = member.as_str().ok_or_else(|| {
                    anyhow::anyhow!("the native IAM baseline has a malformed member")
                })?;
                ensure!(
                    member != intent.binding.member
                        && !member.starts_with(&format!("deleted:{}?uid=", intent.binding.member)),
                    "a preexisting binding names the disposable identity; it was preserved"
                );
            }
        }
        job::append(path, &JobEvent::Intent(Box::new(intent.clone())))?;
        self.cli.run(&["iam", "service-accounts", "create", &identity, &format!("--display-name={identity}"), &format!("--description={}", intent.description())])
            .map_err(|_| anyhow::anyhow!("disposable identity creation failed or its response was lost; its durable intent remains for cleanup"))?;
        let found = self
            .cli
            .json(&[
                "iam",
                "service-accounts",
                "describe",
                &email,
                "--format=json",
            ])
            .map_err(|_| {
                anyhow::anyhow!(
                    "disposable identity observation failed; its durable intent remains for cleanup"
                )
            })?;
        ensure!(
            found["email"].as_str() == Some(&email)
                && found["description"].as_str() == Some(&intent.description()),
            "disposable identity nonce differs; preserve the unexpected identity"
        );
        let account = ServiceAccount {
            unique_id: string(&found, "uniqueId")?,
            description: intent.description(),
        };
        job::append(
            path,
            &JobEvent::Account {
                version: 1,
                nonce: intent.nonce.clone(),
                account,
            },
        )?;
        self.cli.grant(&["projects", "add-iam-policy-binding", &self.project, &format!("--member={}", intent.binding.member), &format!("--role={}", intent.binding.role),
            &format!("--condition=expression={},title={}", intent.binding.expression, intent.binding.title)])
            .map_err(|_| anyhow::anyhow!("disposable identity grant failed or its response was lost; its durable intent remains for cleanup"))?;
        job::append(
            path,
            &JobEvent::Binding {
                version: 1,
                nonce: intent.nonce,
            },
        )?;
        Ok(email)
    }
}

fn string(value: &Value, name: &str) -> anyhow::Result<String> {
    value[name]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("native identity has no {name}"))
}

fn absent(text: &str, operation: &str) -> bool {
    let prefix = format!("ERROR: (gcloud.{operation}) ");
    let lines = text.lines().collect::<Vec<_>>();
    match lines.as_slice() {
        [line] => line
            .strip_prefix(&prefix)
            .is_some_and(|s| s.starts_with("NOT_FOUND:") && !s.contains("PERMISSION_DENIED")),
        [header, detail] => {
            *header == format!("{prefix}Could not fetch resource:")
                && detail.trim_start().starts_with("- The resource '")
                && detail.ends_with("was not found")
        }
        _ => false,
    }
}
