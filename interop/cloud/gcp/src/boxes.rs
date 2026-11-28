//! Boxes on Google Cloud: Compute Engine instances, made with `gcloud`.
//!
//! Each box is an Ubuntu LTS instance in the default network, named
//! `lnk-<box id>` and labelled with its name, ID and owner, reached over
//! SSH with Link's certificates, which its first boot trusts; its
//! `ssh-keys` metadata holds no key of Link's but the one another
//! computer adds while it trusts that computer's too. The host keys its
//! first boot pinned are kept in its metadata too (`lnk-host-keys`). Link
//! makes two more things per project, shared by its boxes and deleted
//! with the last one: firewall rules for machines tagged `lnk-box`, one
//! letting SSH in (`lnk-box-ssh`) and one refusing everything else
//! (`lnk-box-deny`), ahead of the default network's own rules, which let
//! in every port from inside the network, RDP and ping.

#[path = "attachments.rs"]
mod attachments;
mod ending;
mod machines;
use ending::without_poweroff_timer;
use link_format::WithinExt;
use machines::{ARM, size_of, with_attached};
pub use machines::{image, machine_args, split_size};

use std::collections::BTreeMap;

use anyhow::{Context, ensure};

use crate::cli::{Gcp, both};
use link_cloud_contract::{Found, MachineSize, Spec};
use link_format::{Report, aside, sentence};

use link_cloud_contract::ownership::firewall;
/// What a refused GPU or spot box most often needs.
/// The network Google makes in every project, where a box whose spec
/// attaches none goes, behind Link's own firewall rules.
const DEFAULT_NETWORK: &str = "default";

const QUOTA: &str = "Google Cloud gives a new account no GPUs, and few spot machines, until you \
                     ask it to raise the project's quota: in its console's IAM & Admin, Quotas.";

impl Gcp {
    /// Makes a box from `spec`, its disk from the project's `image` if
    /// one is given, else from its OS's.
    pub fn launch(&self, spec: &Spec, image: Option<&str>) -> anyhow::Result<String> {
        // GPUs attached by the size's name are the box's, pinned or picked:
        // its image is the one with NVIDIA's driver.
        let spec = &with_attached(spec)?;
        spec.check()?;
        if link_cloud_contract::ownership::job::journal().is_some()
            && spec.attach().network.is_none()
        {
            self.firewall()?;
        }
        let journaled_identity =
            if spec.self_remove() && link_cloud_contract::ownership::job::journal().is_some() {
                Some(self.job_identity(spec)?)
            } else {
                None
            };
        let attached = self.attachment_args(spec.attach())?;
        if image.is_none() {
            self::image(spec)?;
        }
        // Its image and the firewall's rules don't need each other: asked
        // (the rules made where missing) side by side.
        let (source, firewall) = both(
            || -> anyhow::Result<(Vec<String>, Option<u64>)> {
                // A saved image's disk size is the box module's to raise
                // to, from `image-list`; an OS's current image is asked here.
                Ok(match image {
                    Some(id) => (vec!["--image".to_string(), self.launch_image(id)?], None),
                    None => {
                        let (family, project) = self::image(spec)?;
                        // The disk is at least the image's, or Google
                        // refuses to make it.
                        let image_gb = self.image_disk_gb(family, project);
                        let source = ["--image-family", family, "--image-project", project]
                            .map(String::from)
                            .to_vec();
                        (source, image_gb)
                    }
                })
            },
            || {
                if spec.attach().network.is_some() {
                    Ok(())
                } else {
                    self.firewall()
                }
            },
        );
        let (source, image_gb) = source?;
        firewall?;
        if let Some(gb) = image_gb.filter(|gb| *gb > spec.disk_gb()) {
            aside(Report::warning(sentence!(
                "Its image needs a disk of {} GB at least: the box gets {} GB, not {}.",
                gb,
                gb,
                spec.disk_gb()
            )));
        }
        let spec = &Spec {
            disk_gb: spec
                .disk_gb
                .max(image_gb.and_then(|gb| u16::try_from(gb).ok()).unwrap_or(0)),
            ..spec.clone()
        };
        let machine = machine_args(spec)?;
        let identity = if let Some(identity) = journaled_identity {
            Some(identity)
        } else if spec.self_remove() {
            Some(self.job_identity(spec)?)
        } else {
            None
        };
        let init = if spec.self_remove() {
            self.ending_init(&spec.user_data, spec)?
        } else {
            spec.user_data.clone()
        };
        let init = without_poweroff_timer(&init);
        let user_data = crate::cli::private_file("user-data", &init)?;
        let name = link_cloud_contract::machine_name(spec);
        let (machine_type, _) = split_size(&spec.size)?;
        let mut tags = spec.tags.clone();
        tags.entry("lnk-box".into())
            .or_insert_with(|| spec.name.clone());
        // Labels, never network tags: the firewall's rules match those,
        // and the only one a box has is Link's own.
        let labels = labels(&tags)?;
        let user_data = format!("user-data={}", user_data.path().display());
        // No key of the project's, nor OS Login's: only what its first
        // boot trusts (an older `lnk box`'s key, when it gives one).
        let mut metadata = "enable-oslogin=FALSE,block-project-ssh-keys=TRUE".to_string();
        let key = spec.public_key().trim();
        if !key.is_empty() {
            ensure!(!key.contains(['\n', '\r']), "that isn't one public key");
            metadata = format!("ssh-keys=ubuntu:{key},{metadata}");
        }
        let mut args = vec![
            "compute",
            "instances",
            "create",
            &name,
            "--zone",
            &self.zone,
            "--machine-type",
            machine_type,
            // No identity: the default service account's token, which
            // any program on the box (an agent included) could read from
            // the metadata service, reaches the project's storage. Link
            // acts on the cloud as your account, never as the box.
            "--no-service-account",
            "--no-scopes",
            "--tags",
            firewall::tag(),
            "--labels",
            &labels,
            "--metadata-from-file",
            &user_data,
            // Only Link's key: never the project's own SSH keys, which
            // anyone who can edit the project's metadata adds.
            "--metadata",
            &metadata,
            "--format",
            "json",
        ];
        if spec.attach().identity.is_some() {
            args.retain(|arg| !matches!(*arg, "--no-service-account" | "--no-scopes"));
        }
        args.extend(attached.iter().map(String::as_str));
        if let Some(identity) = &identity {
            args.retain(|arg| !matches!(*arg, "--no-service-account" | "--no-scopes"));
            args.extend([
                "--service-account",
                identity,
                "--scopes",
                "https://www.googleapis.com/auth/compute",
            ]);
        }
        args.extend(source.iter().map(String::as_str));
        args.extend(machine.iter().map(String::as_str));
        self.cli
            .run(&args)
            .map_err(|err| match spec.gpus > 0 || spec.spot {
                true => anyhow::anyhow!("{err:#}\n\n{QUOTA}"),
                false => err,
            })?;
        Ok(name)
    }

    fn describe(&self, id: &str) -> anyhow::Result<serde_json::Value> {
        self.cli.json(&[
            "compute",
            "instances",
            "describe",
            id,
            "--zone",
            &self.zone,
            "--format",
            "json",
        ])
    }

    pub fn wait_running(&self, id: &str) -> anyhow::Result<String> {
        // `instances create` returns once it runs.
        let instance = self.describe(id)?;
        Ok(address(&instance)
            .within(format!("{id} has no address to reach it at"))?
            .to_string())
    }

    pub fn start(&self, id: &str) -> anyhow::Result<String> {
        self.cli
            .run(&["compute", "instances", "start", id, "--zone", &self.zone])?;
        self.wait_running(id)
    }

    pub fn stop(&self, id: &str) -> anyhow::Result<()> {
        self.cli
            .run(&["compute", "instances", "stop", id, "--zone", &self.zone])?;
        Ok(())
    }

    pub fn state(&self, id: &str) -> anyhow::Result<String> {
        match self.describe(id) {
            Ok(instance) => Ok(instance["status"]
                .as_str()
                .within("Google returned no instance state")?
                .to_lowercase()),
            Err(err) if err.to_string().contains("was not found") => Ok("gone".into()),
            Err(err) => Err(err),
        }
    }

    pub fn remove(&self, id: &str) -> anyhow::Result<()> {
        match self.cli.run(&[
            "compute",
            "instances",
            "delete",
            id,
            "--zone",
            &self.zone,
            "--delete-disks",
            "boot",
            "--quiet",
        ]) {
            Err(err) if err.to_string().contains("was not found") => Ok(()),
            other => other.map(|_| ()),
        }
    }

    /// Its metadata item `key`, if it has one.
    fn metadata(&self, id: &str, key: &str) -> anyhow::Result<Option<String>> {
        let instance = self.describe(id)?;
        Ok(instance["metadata"]["items"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|item| item["key"] == key)
            .and_then(|item| item["value"].as_str())
            .map(str::to_string))
    }

    /// Sets its metadata item `key` to `value` (through a file: values
    /// span lines), leaving the others.
    fn set_metadata(&self, id: &str, key: &str, value: &str) -> anyhow::Result<()> {
        let file = crate::cli::private_file(key, value)?;
        self.cli.run(&[
            "compute",
            "instances",
            "add-metadata",
            id,
            "--zone",
            &self.zone,
            "--metadata-from-file",
            &format!("{key}={}", file.path().display()),
        ])?;
        Ok(())
    }

    /// The host keys kept on it (`lnk-host-keys`), else those its first
    /// boot printed on the serial console.
    pub fn host_keys(&self, id: &str) -> anyhow::Result<Vec<String>> {
        let kept: Vec<String> = self
            .metadata(id, "lnk-host-keys")?
            .unwrap_or_default()
            .lines()
            .filter_map(link_cloud_contract::host_key)
            .collect();
        if !kept.is_empty() {
            return Ok(kept);
        }
        self.console_host_keys(id)
    }

    /// Keeps `keys` on it, for another computer to pin.
    pub fn save_host_keys(&self, id: &str, keys: &[String]) -> anyhow::Result<()> {
        let keys: Vec<String> = keys
            .iter()
            .filter_map(|k| link_cloud_contract::host_key(k))
            .collect();
        ensure!(!keys.is_empty(), "no host keys to keep");
        self.set_metadata(id, "lnk-host-keys", &keys.join("\n"))
    }

    /// Lets `public_key` in as `ubuntu`, merged with the keys it has:
    /// never replaced.
    pub fn add_key(&self, id: &str, public_key: &str) -> anyhow::Result<()> {
        let key = public_key.trim();
        ensure!(
            !key.is_empty() && !key.contains(['\n', '\r']),
            "that isn't one public key"
        );
        let line = format!("ubuntu:{key}");
        let mut lines: Vec<String> = self
            .metadata(id, "ssh-keys")?
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect();
        if lines.contains(&line) {
            return Ok(());
        }
        lines.push(line);
        self.set_metadata(id, "ssh-keys", &lines.join("\n"))
    }

    /// Link's own keys gone from its `ssh-keys`
    /// ([`link_cloud_contract::links_ssh_key`]), anyone else's kept: the
    /// guest agent then takes them out of `authorized_keys` too.
    pub fn remove_keys(&self, id: &str) -> anyhow::Result<()> {
        let Some(keys) = self.metadata(id, "ssh-keys")? else {
            return Ok(());
        };
        let kept: Vec<&str> = keys
            .lines()
            .filter(|l| !l.trim().is_empty() && !link_cloud_contract::links_ssh_key(l))
            .collect();
        if kept.len() == keys.lines().filter(|l| !l.trim().is_empty()).count() {
            return Ok(());
        }
        self.set_metadata(id, "ssh-keys", &kept.join("\n"))
    }

    /// Adds `tags` to its labels.
    pub fn tag(&self, id: &str, tags: &BTreeMap<String, String>) -> anyhow::Result<()> {
        self.cli.run(&[
            "compute",
            "instances",
            "add-labels",
            id,
            "--zone",
            &self.zone,
            "--labels",
            &labels(tags)?,
        ])?;
        Ok(())
    }

    /// Every box in the project labelled `lnk-owner=<owner>`, in any
    /// zone; with no owner, every box labelled `lnk-owner` at all.
    pub fn list(&self, owner: Option<&str>) -> anyhow::Result<Vec<Found>> {
        let filter = match owner {
            Some(owner) => {
                label_value(owner)?;
                format!("labels.lnk-owner={owner}")
            }
            None => "labels.lnk-owner:*".to_string(),
        };
        let found = self.cli.json(&[
            "compute",
            "instances",
            "list",
            "--filter",
            &filter,
            "--format",
            "json",
        ])?;
        Ok(found
            .as_array()
            .into_iter()
            .flatten()
            .map(found_of)
            .collect())
    }

    /// The box as Google Cloud says it is: its type and zone, whether
    /// it's spot, its boot disk (`disks describe`) and its OS (the disk's
    /// licenses).
    pub fn describe_box(&self, id: &str) -> anyhow::Result<Found> {
        let instance = self.describe(id)?;
        let mut found = found_of(&instance);
        found.spot = Some(instance["scheduling"]["provisioningModel"] == "SPOT");
        let family = found.size.split('-').next().unwrap_or_default();
        found.arch = Some(
            if ARM.contains(&family) {
                "arm64"
            } else {
                "x86_64"
            }
            .into(),
        );
        let boot = instance["disks"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["boot"] == true);
        if let Some(boot) = boot {
            found.disk_gb = boot["diskSizeGb"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .or(boot["diskSizeGb"].as_u64());
            found.os = boot["licenses"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l.as_str())
                .find_map(|l| {
                    let license = l.rsplit('/').next().unwrap_or_default();
                    link_cloud_contract::OSES.iter().find(|os| {
                        license.contains(&os.trim_start_matches("ubuntu-").replace('.', ""))
                    })
                })
                .map(|os| os.to_string());
            let disk = last(&boot["source"]);
            if let Ok(disk) = self.cli.json(&[
                "compute", "disks", "describe", &disk, "--zone", &self.zone, "--format", "json",
            ]) {
                found.disk_type = match last(&disk["type"]).as_str() {
                    "pd-balanced" => Some("balanced".into()),
                    "pd-ssd" => Some("fast".into()),
                    _ => None,
                };
            }
        }
        Ok(found)
    }

    /// The SSH host keys its first boot printed on the serial console.
    fn console_host_keys(&self, id: &str) -> anyhow::Result<Vec<String>> {
        let console = self.cli.run(&[
            "compute",
            "instances",
            "get-serial-port-output",
            id,
            "--zone",
            &self.zone,
        ])?;
        Ok(link_cloud_contract::host_keys(&String::from_utf8_lossy(
            &console,
        )))
    }

    /// Deletes the firewall rules, unless a Link box anywhere in the
    /// project still uses it (asked of Google, not remembered).
    pub fn cleanup(&self) -> anyhow::Result<()> {
        // A journaled launch borrows project setup and never owns its rules.
        if link_cloud_contract::ownership::job::journal().is_some() {
            return Ok(());
        }
        let left = self.cli.json(&[
            "compute",
            "instances",
            "list",
            "--filter",
            "labels.lnk-box:*",
            "--format",
            "json",
        ])?;
        if left.as_array().is_some_and(|l| !l.is_empty()) {
            return Ok(());
        }
        for rule in firewall::rules().iter().rev() {
            if self.find_firewall(&rule.name) {
                self.cli
                    .run(&["compute", "firewall-rules", "delete", &rule.name, "--quiet"])?;
            }
        }
        Ok(())
    }

    fn find_firewall(&self, rule: &str) -> bool {
        self.cli
            .run(&[
                "compute",
                "firewall-rules",
                "describe",
                rule,
                "--format",
                "json",
            ])
            .is_ok()
    }

    /// The rules letting SSH in to Link's boxes and nothing else
    /// ([`firewall::rules`]), made the first time in their order, the deny
    /// rule first, so a box is never let in on more than SSH.
    fn firewall(&self) -> anyhow::Result<()> {
        let rules = firewall::rules();
        let found: Vec<bool> = rules.iter().map(|r| self.find_firewall(&r.name)).collect();
        if link_cloud_contract::ownership::job::journal().is_some() {
            ensure!(
                found.iter().all(|f| *f),
                "journaled launches require both shared Link firewall rules to be preprovisioned; no rule was created"
            );
            return Ok(());
        }
        for (rule, there) in rules.iter().zip(found) {
            if there {
                continue;
            }
            let priority = rule.priority.to_string();
            made_once(self.cli.run(&[
                "compute",
                "firewall-rules",
                "create",
                &rule.name,
                "--network",
                DEFAULT_NETWORK,
                "--action",
                rule.action.word(),
                "--rules",
                &rule.rules.join(","),
                "--source-ranges",
                &rule.sources.join(","),
                "--priority",
                &priority,
                "--target-tags",
                firewall::tag(),
                "--description",
                &rule.description,
                "--format",
                "json",
            ]))?;
        }
        Ok(())
    }
}

/// Where instance `i` is reached: its public address on the project's
/// `default` network, where Link's own firewall rules let SSH in, and its
/// private one on a network its spec attached, whose rules let in only
/// its peers on that network.
fn address(i: &serde_json::Value) -> Option<&str> {
    let interface = &i["networkInterfaces"][0];
    if last(&interface["network"]) == DEFAULT_NETWORK {
        interface["accessConfigs"][0]["natIP"].as_str()
    } else {
        interface["networkIP"].as_str()
    }
}

/// The last part of a URL Google gives (`.../zones/europe-west1-b`).
pub(crate) fn last(url: &serde_json::Value) -> String {
    url.as_str()
        .and_then(|u| u.rsplit('/').next())
        .unwrap_or_default()
        .to_string()
}

/// A box as `instances list` or `describe` says it is.
fn found_of(i: &serde_json::Value) -> Found {
    Found {
        tags: i["labels"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
            .collect(),
        id: i["name"].as_str().unwrap_or_default().to_string(),
        name: i["labels"]["lnk-box"].as_str().unwrap_or_default().into(),
        machine_id: i["labels"]["lnk-machine"]
            .as_str()
            .unwrap_or_default()
            .into(),
        owner: i["labels"]["lnk-owner"]
            .as_str()
            .map(str::to_string)
            .filter(|o| !o.is_empty()),
        deadline: i["labels"]["lnk-deadline"]
            .as_str()
            .and_then(|s| s.parse().ok()),
        place: last(&i["zone"]),
        state: i["status"].as_str().unwrap_or("unknown").to_lowercase(),
        ip: address(i).map(str::to_string),
        size: size_of(i),
        ..Found::default()
    }
}

/// `tags` as gcloud's `--labels`: `k=v,k=v`, each a valid label.
pub(crate) fn labels(tags: &BTreeMap<String, String>) -> anyhow::Result<String> {
    let mut all = Vec::new();
    for (key, value) in tags {
        label_value(key)?;
        label_value(value)?;
        all.push(format!("{key}={value}"));
    }
    Ok(all.join(","))
}

/// An error unless `text` can be a label's key or value (and so can't
/// widen a filter): lowercase letters, digits, `-` and `_`, at most 63.
pub(crate) fn label_value(text: &str) -> anyhow::Result<()> {
    ensure!(
        text.len() <= 63
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'),
        "{:?} can't be a label",
        link_format::clean(text)
    );
    Ok(())
}

/// A shared resource's creation, made by this launch or by another one at
/// the same time: boxes launched at once each find no rule and make it,
/// and all but the first are told it already exists.
fn made_once(created: anyhow::Result<Vec<u8>>) -> anyhow::Result<()> {
    match created {
        Err(err) if err.to_string().contains("already exists") => Ok(()),
        other => other.map(drop),
    }
}

#[cfg(test)]
mod tests {
    /// A box on the default network is reached at its public address, and
    /// one on a network its spec attached at its private one.
    #[test]
    fn a_box_on_an_attached_network_is_reached_privately() {
        let on = |network: &str| {
            serde_json::json!({ "networkInterfaces": [{
                "network": format!("https://www.googleapis.com/compute/v1/projects/p/global/networks/{network}"),
                "networkIP": "10.128.0.7",
                "accessConfigs": [{ "natIP": "34.1.2.3" }],
            }]})
        };
        assert_eq!(super::address(&on("default")), Some("34.1.2.3"));
        assert_eq!(super::address(&on("lnkc-boxes")), Some("10.128.0.7"));
    }

    /// A rule another launch made meanwhile is made; any other failure isn't.
    #[test]
    fn a_rule_made_meanwhile_is_made() {
        let exists = anyhow::anyhow!(
            "ERROR: (gcloud.compute.firewall-rules.create) Could not fetch resource: - \
             The resource 'projects/p/global/firewalls/lnk-box-deny' already exists"
        );
        assert!(super::made_once(Err(exists)).is_ok());
        assert!(super::made_once(Ok(Vec::new())).is_ok());
        assert!(super::made_once(Err(anyhow::anyhow!("PERMISSION_DENIED"))).is_err());
    }
}
