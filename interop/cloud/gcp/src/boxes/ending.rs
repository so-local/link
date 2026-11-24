//! A box that ends itself: its own job identity, granted only on its VM,
//! its cloud deadline in its first boot, and the identities reclaimed
//! once their VM is gone.

use super::*;
mod journaled;

impl Gcp {
    /// One job identity, whose only grant matches this VM's resource name.
    pub(super) fn job_identity(&self, spec: &Spec) -> anyhow::Result<String> {
        if let Some(path) = link_cloud_contract::ownership::job::journal() {
            return self.journaled_job_identity(&path, spec);
        }
        let project = &self.project;
        link_machine::machine::check_id(spec.id())?;
        let name = link_cloud_contract::machine_name(spec);
        self.reap_job_identities(project)?;
        let identity = format!("lnk-job-{}", spec.id());
        let email = format!("{identity}@{project}.iam.gserviceaccount.com");
        let role = format!("projects/{project}/roles/lnkBoxEnd");
        // The role is read first, so an account that may only grant it
        // needs no right to make roles; one granting anything else is
        // refused.
        let describe = |cli: &crate::cli::Cli| {
            cli.json(&[
                "iam",
                "roles",
                "describe",
                "lnkBoxEnd",
                &format!("--project={project}"),
                "--format=json",
            ])
        };
        let only_delete = |role: &serde_json::Value| {
            ensure!(
                role["includedPermissions"] == serde_json::json!(["compute.instances.delete"]),
                "lnkBoxEnd must grant only compute.instances.delete"
            );
            Ok(())
        };
        match describe(&self.cli) {
            Ok(existing) => only_delete(&existing)?,
            Err(err)
                if err.to_string().contains("NOT_FOUND")
                    || err.to_string().contains("was not found") =>
            {
                match self.cli.run(&[
                    "iam",
                    "roles",
                    "create",
                    "lnkBoxEnd",
                    &format!("--project={project}"),
                    "--permissions=compute.instances.delete",
                    "--stage=GA",
                ]) {
                    Ok(_) => {}
                    // Another launch made it meanwhile.
                    Err(err)
                        if err.to_string().contains("ALREADY_EXISTS")
                            || err.to_string().contains("already exists") =>
                    {
                        only_delete(&describe(&self.cli)?)?
                    }
                    Err(err) => return Err(err),
                }
            }
            Err(err) => return Err(err),
        }
        self.cli.run(&[
            "iam",
            "service-accounts",
            "create",
            &identity,
            &format!("--display-name={identity}"),
            &format!("--description=Link job box {}/{name}", self.zone),
        ])?;
        let resource = format!("projects/{project}/zones/{}/instances/{name}", self.zone);
        self.cli.grant(&[
            "projects",
            "add-iam-policy-binding",
            project,
            &format!("--member=serviceAccount:{email}"),
            &format!("--role={role}"),
            &format!("--condition=expression=resource.name == '{resource}',title={identity}"),
        ])?;
        Ok(email)
    }

    pub(super) fn ending_init(&self, init: &str, spec: &Spec) -> anyhow::Result<String> {
        let project = &self.project;
        let policy = link_cloud_contract::ending::Policy {
            credential_url: "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token".into(),
            credential_headers: BTreeMap::from([("Metadata-Flavor".into(), "Google".into())]),
            remove_url: format!("https://compute.googleapis.com/compute/v1/projects/{project}/zones/{}/instances/{}", self.zone, link_cloud_contract::machine_name(spec)),
        };
        let content = serde_json::to_string(&policy)?;
        let end = link_cloud_contract::first_boot::file(
            &link_files::layout::System::BoxEnd
                .path()
                .display()
                .to_string(),
            &[&content],
        );
        // A GCP job must reach the cloud deletion deadline even if self-removal fails:
        // never install the guest poweroff timer, which would clear that deadline.
        let init = without_poweroff_timer(init);
        Ok(link_cloud_contract::first_boot::with(&init, &end, ""))
    }
}

/// Stopping a GCP VM clears its cloud deadline: the cloud alone ends it.
pub(super) fn without_poweroff_timer(init: &str) -> String {
    link_cloud_contract::first_boot::without_deadline(init)
}

impl Gcp {
    /// Reclaim only the identities this adapter made, after their VM is confirmed gone.
    /// A dead starter is never needed to reclaim them: the next disposable launch does it.
    fn reap_job_identities(&self, project: &str) -> anyhow::Result<()> {
        let accounts = self.cli.json(&[
            "iam",
            "service-accounts",
            "list",
            "--filter=displayName:lnk-job-",
            "--format=json",
        ])?;
        for account in accounts.as_array().into_iter().flatten() {
            let Some(identity) = account["displayName"]
                .as_str()
                .filter(|s| s.starts_with("lnk-job-"))
            else {
                continue;
            };
            let id = identity.trim_start_matches("lnk-job-");
            if link_machine::machine::check_id(id).is_err() {
                continue;
            }
            let email = format!("{identity}@{project}.iam.gserviceaccount.com");
            if account["email"].as_str() != Some(email.as_str()) {
                continue;
            }
            let Some((zone, name)) = account["description"]
                .as_str()
                .and_then(|s| s.strip_prefix("Link job box "))
                .and_then(|s| s.split_once('/'))
            else {
                continue;
            };
            if name != format!("lnk-{id}") || label_value(zone).is_err() {
                continue;
            }
            match self.cli.run(&[
                "compute",
                "instances",
                "describe",
                name,
                "--zone",
                zone,
                "--format=json",
            ]) {
                Ok(_) => continue,
                Err(err)
                    if err.to_string().contains("was not found")
                        || err.to_string().contains("NOT_FOUND") => {}
                Err(err) => return Err(err),
            }
            let resource = format!("projects/{project}/zones/{zone}/instances/{name}");
            // Another launch may reclaim it at the same time: gone is done.
            gone(self.cli.grant(&[
                "projects",
                "remove-iam-policy-binding",
                project,
                &format!("--member=serviceAccount:{email}"),
                &format!("--role=projects/{project}/roles/lnkBoxEnd"),
                &format!("--condition=expression=resource.name == '{resource}',title={identity}"),
            ]))?;
            // Google says an account deleted meanwhile is one denied:
            // asked again, an account not there is reclaimed.
            let deleted =
                gone(
                    self.cli
                        .run(&["iam", "service-accounts", "delete", &email, "--quiet"]),
                );
            if let Err(err) = deleted
                && self
                    .cli
                    .run(&["iam", "service-accounts", "describe", &email])
                    .is_ok()
            {
                return Err(err);
            }
        }
        Ok(())
    }
}

/// An identity's reclaiming, done by this launch or by another one at the
/// same time, which leaves this one's binding or account not found.
fn gone(reclaimed: anyhow::Result<Vec<u8>>) -> anyhow::Result<()> {
    match reclaimed {
        Err(err)
            if err.to_string().contains("not found") || err.to_string().contains("NOT_FOUND") =>
        {
            Ok(())
        }
        other => other.map(drop),
    }
}

#[cfg(test)]
mod reclaiming {
    /// What another launch reclaimed first is reclaimed; a refusal isn't.
    #[test]
    fn an_identity_reclaimed_meanwhile_is_reclaimed() {
        let gone = anyhow::anyhow!(
            "ERROR: (gcloud.projects.remove-iam-policy-binding) Policy binding with the \
             specified principal, role, and condition not found!"
        );
        assert!(super::gone(Err(gone)).is_ok());
        assert!(super::gone(Err(anyhow::anyhow!("PERMISSION_DENIED"))).is_err());
    }
}
