//! Boxes on AWS: EC2 instances, made with the `aws` CLI.
//!
//! Each box is an Ubuntu LTS instance in the account's default VPC,
//! named `lnk-<box id>` and tagged with its name, ID and owner, reached
//! over SSH with Link's certificates, which its first boot trusts; another
//! computer's key is pushed with EC2 Instance Connect for a minute, while
//! `lnk box` has the box trust that computer's certificates too. The
//! host key its first boot pinned is kept in a tag (`lnk-host-key`). Link
//! makes a security group per region, shared by its boxes there and
//! deleted with the last one (`lnk-box`, SSH in; `lnk-box-tunnel`, nothing
//! in, for a box reached only through Systems Manager). A key pair
//! (`lnk-box-<hash>`) is made only for an older `lnk box`, which gives a
//! key, and deleted the same way.

#[path = "attachments.rs"]
mod attachments;
#[path = "groups.rs"]
mod groups;
mod machines;
mod shared;
use link_format::WithinExt;
use machines::ROOT;
pub use machines::{disk, image_parameter, image_root, spot_options, tag_specifications};

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::{Context, bail, ensure};

use crate::cli::{Aws, both};
use link_cloud_contract::{Found, MachineSize, Spec};
use link_format::{Report, aside, sentence};

/// What a refused GPU or spot box most often needs.
const QUOTA: &str = "AWS gives a new account no GPUs, and few spot machines, until you \
                     ask it to raise the account's quota: in its console's Service Quotas (EC2).";
/// How many regions [`Aws::list`] asks at once.
const LIST_AT_ONCE: usize = 8;
/// How often a wait asks AWS for an instance's state again (the CLI's
/// own `aws ec2 wait` asks every 15 s), and for how long at most, as the
/// CLI's.
const POLL: Duration = Duration::from_secs(3);
const WAIT: Duration = Duration::from_secs(600);

impl Aws {
    /// The image a box of `spec` starts from (the account's `image` if
    /// one is given, else its OS's current one), its root device, and the
    /// disk in GB it needs at least, when AWS says.
    fn launch_source(
        &self,
        spec: &Spec,
        image: Option<&str>,
    ) -> anyhow::Result<(String, String, Option<u64>)> {
        // A saved image's disk size is the box module's to raise to, from
        // `image-list`; an OS's current image is asked here.
        if let Some(id) = image {
            let (image, device) = self.launch_image(id)?;
            return Ok((image, device, None));
        }
        let parameter = image_parameter(spec)?;
        let image = self
            .cli
            .json(&["ssm", "get-parameter", "--name", parameter])?["Parameter"]["Value"]
            .as_str()
            .within("AWS didn't say which Ubuntu image is current")?
            .to_string();
        // The disk is at least the image's: an image AWS won't describe
        // gets the one asked, and AWS says if it's too small.
        let (device, image_gb) =
            match self
                .cli
                .json(&["ec2", "describe-images", "--image-ids", &image])
            {
                Ok(images) => image_root(&images),
                Err(_) => (ROOT.to_string(), None),
            };
        Ok((image, device, image_gb))
    }

    /// Makes a box from `spec`, its disk from the account's `image` if
    /// one is given, else from its OS's.
    pub fn launch(&self, spec: &Spec, image: Option<&str>) -> anyhow::Result<String> {
        // Its image, key pair and security group don't need each other:
        // asked (and made where missing) side by side, once the spec is
        // one AWS can make.
        spec.check()?;
        let attached = self.attachment_args(spec.attach())?;
        if image.is_none() {
            image_parameter(spec)?;
        }
        let ((source, key), group) = both(
            || {
                both(
                    || self.launch_source(spec, image),
                    || match spec.public_key().trim() {
                        "" => Ok(None),
                        key => self.key_pair(key).map(Some),
                    },
                )
            },
            || {
                if spec.attach().network.is_some() {
                    Ok(String::new())
                } else {
                    self.security_group(spec.tunnel())
                }
            },
        );
        let (image, device, image_gb) = source?;
        let (key, group) = (key?, group?);
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
        let user_data = crate::cli::private_file("user-data", &spec.user_data)?;
        let mut all = spec.tags.clone();
        all.entry("lnk-box".into())
            .or_insert_with(|| spec.name.clone());
        all.insert("Name".into(), link_cloud_contract::machine_name(spec));
        let tags = tag_specifications(&all, spec.spot)?;
        let disk = disk(spec, &device)?;
        let user_data = format!("file://{}", user_data.path().display());
        let mut args = vec![
            "ec2",
            "run-instances",
            "--image-id",
            &image,
            "--instance-type",
            &spec.size,
            "--security-group-ids",
            &group,
            "--user-data",
            &user_data,
            // Instance metadata only with a session token (IMDSv2).
            "--metadata-options",
            "HttpTokens=required,HttpEndpoint=enabled",
            "--block-device-mappings",
            &disk,
            "--tag-specifications",
            &tags,
            "--count",
            "1",
        ];
        if spec.attach().network.is_some()
            && let Some(at) = args.iter().position(|a| *a == "--security-group-ids")
        {
            args.drain(at..at + 2);
        }
        args.extend(attached.iter().map(String::as_str));
        if let Some(key) = &key {
            args.extend(["--key-name", key]);
        }
        if spec.deadline.is_some() {
            args.extend(["--instance-initiated-shutdown-behavior", "terminate"]);
        }
        let spot = spot_options();
        if spec.spot {
            args.extend(["--instance-market-options", &spot]);
        }
        let launched = self
            .cli
            .json(&args)
            .map_err(|err| match spec.gpus > 0 || spec.spot {
                true => anyhow::anyhow!("{err:#}\n\n{QUOTA}"),
                false => err,
            })?;
        let id = launched["Instances"][0]["InstanceId"]
            .as_str()
            .within("AWS didn't say which instance it made")?;
        if let Err(err) = self.attach_existing(id, spec.attach()) {
            let cleanup = self.remove(id);
            return Err(err.context(format!(
                "attaching resources to {id}; VM rollback: {cleanup:?}"
            )));
        }
        Ok(id.to_string())
    }

    pub fn wait_running(&self, id: &str) -> anyhow::Result<String> {
        // Just made, AWS may not list it yet: "gone" is waited out.
        let instance = self
            .wait_for(
                id,
                &["running"],
                &["shutting-down", "terminated", "stopping"],
            )
            .within(format!("waiting for {id} to run"))?;
        address_of(id, &instance)
    }

    /// Instance `id` once its state is one of `done`, asked every
    /// [`POLL`] for up to [`WAIT`]: an error once it's in one of `failed`,
    /// as the CLI's waiters have it. One AWS doesn't know is "gone".
    #[expect(
        clippy::disallowed_methods,
        reason = "polls AWS for an instance's state, which it doesn't push"
    )]
    fn wait_for(
        &self,
        id: &str,
        done: &[&str],
        failed: &[&str],
    ) -> anyhow::Result<serde_json::Value> {
        let start = Instant::now();
        loop {
            let instance = match self.describe(id) {
                Ok(instance) => instance,
                Err(err) if err.to_string().contains("InvalidInstanceID.NotFound") => {
                    serde_json::Value::Null
                }
                Err(err) => return Err(err),
            };
            let state = instance["State"]["Name"].as_str().unwrap_or("gone");
            if done.contains(&state) {
                return Ok(instance);
            }
            ensure!(
                !failed.contains(&state),
                "{id} is {}, not {}",
                link_format::clean(state),
                done[0]
            );
            ensure!(
                start.elapsed() < WAIT,
                "{id} wasn't {} within {} s",
                done[0],
                WAIT.as_secs()
            );
            std::thread::sleep(POLL);
        }
    }

    fn describe(&self, id: &str) -> anyhow::Result<serde_json::Value> {
        let found = self
            .cli
            .json(&["ec2", "describe-instances", "--instance-ids", id])?;
        Ok(found["Reservations"][0]["Instances"][0].clone())
    }

    pub fn start(&self, id: &str) -> anyhow::Result<String> {
        // AWS doesn't start a spot machine it took back: its persistent
        // request does, once AWS has the capacity.
        if stopped_by_aws(&self.describe(id)?) == Some(true) {
            bail!(
                "AWS took this spot machine back, and starts it again itself once it has the \
                 capacity, as its spot request asks: lnk box list --refresh shows when it runs"
            );
        }
        self.cli
            .run(&["ec2", "start-instances", "--instance-ids", id])?;
        self.wait_running(id)
    }

    pub fn stop(&self, id: &str) -> anyhow::Result<()> {
        self.cli
            .run(&["ec2", "stop-instances", "--instance-ids", id])?;
        self.wait_for(id, &["stopped"], &["pending", "terminated", "gone"])?;
        Ok(())
    }

    pub fn state(&self, id: &str) -> anyhow::Result<String> {
        Ok(self.state_and_why(id)?.0)
    }

    /// Its state, and whether AWS stopped it taking a spot machine back.
    pub fn state_and_why(&self, id: &str) -> anyhow::Result<(String, Option<bool>)> {
        match self.describe(id) {
            Ok(instance) => Ok((
                match instance["State"]["Name"]
                    .as_str()
                    .within("AWS returned no instance state")?
                {
                    "terminated" => "gone".into(),
                    state => state.to_string(),
                },
                stopped_by_aws(&instance),
            )),
            Err(err) if err.to_string().contains("InvalidInstanceID.NotFound") => {
                Ok(("gone".into(), None))
            }
            Err(err) => Err(err),
        }
    }

    /// The box as AWS says it is: its type and region, whether it's spot,
    /// its disk (`describe-volumes`) and its OS (its image's name).
    pub fn describe_box(&self, id: &str) -> anyhow::Result<Found> {
        let instance = self.describe(id)?;
        let mut found = found_of(&instance, &self.region);
        found.spot = Some(instance["InstanceLifecycle"] == "spot");
        found.arch = instance["Architecture"].as_str().map(str::to_string);
        let volume = instance["BlockDeviceMappings"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|m| m["DeviceName"] == instance["RootDeviceName"])
            .and_then(|m| m["Ebs"]["VolumeId"].as_str());
        let image = instance["ImageId"].as_str();
        // Its disk and its image, side by side.
        let (volumes, images) = both(
            || {
                volume
                    .map(|v| {
                        self.cli
                            .json(&["ec2", "describe-volumes", "--volume-ids", v])
                    })
                    .transpose()
            },
            || image.map(|i| self.cli.json(&["ec2", "describe-images", "--image-ids", i])),
        );
        if let Some(volumes) = volumes? {
            let disk = &volumes["Volumes"][0];
            found.disk_gb = disk["Size"].as_u64();
            found.disk_type = match disk["VolumeType"].as_str() {
                Some("gp3") => Some("balanced".into()),
                Some("io2") => Some("fast".into()),
                _ => None,
            };
        }
        // An image deregistered since says nothing: its OS is left out.
        if let Some(Ok(images)) = images {
            found.os = images["Images"][0]["Name"]
                .as_str()
                .and_then(os_of_image)
                .map(str::to_string);
        }
        Ok(found)
    }

    pub fn remove(&self, id: &str) -> anyhow::Result<()> {
        // A spot box's persistent request first: it would make the box
        // again once its instance is terminated.
        let request = match self.describe(id) {
            Ok(instance) => instance["SpotInstanceRequestId"]
                .as_str()
                .map(str::to_string),
            Err(err) if err.to_string().contains("InvalidInstanceID.NotFound") => None,
            Err(err) => return Err(err),
        };
        if let Some(request) = request {
            self.cancel_spot_requests(&[request])?;
        }
        match self
            .cli
            .run(&["ec2", "terminate-instances", "--instance-ids", id])
        {
            Err(err) if err.to_string().contains("InvalidInstanceID.NotFound") => return Ok(()),
            other => other?,
        };
        // The security group can only go once nothing uses it.
        self.wait_for(id, &["terminated", "gone"], &["pending", "stopping"])?;
        Ok(())
    }

    /// The host key kept on it (its `lnk-host-key` tag), else those its
    /// first boot printed on the console.
    pub fn host_keys(&self, id: &str) -> anyhow::Result<Vec<String>> {
        let instance = self.describe(id)?;
        let kept = instance["Tags"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| t["Key"] == "lnk-host-key")
            .and_then(|t| t["Value"].as_str())
            .and_then(link_cloud_contract::host_key);
        match kept {
            Some(key) => Ok(vec![key]),
            None => self.console_host_keys(id),
        }
    }

    /// Keeps one of `keys` (ed25519 if there is one: a tag holds 256
    /// characters) on it, for another computer to pin.
    pub fn save_host_keys(&self, id: &str, keys: &[String]) -> anyhow::Result<()> {
        let keys: Vec<String> = keys
            .iter()
            .filter_map(|k| link_cloud_contract::host_key(k))
            .filter(|k| k.len() <= 256)
            .collect();
        let key = keys
            .iter()
            .find(|k| k.starts_with("ssh-ed25519 "))
            .or(keys.first())
            .within("no host key short enough to keep in a tag")?;
        self.create_tags(id, &BTreeMap::from([("lnk-host-key".into(), key.clone())]))
    }

    /// Pushes `public_key` for `ubuntu` with EC2 Instance Connect: it lets
    /// that key in for a minute, while the caller appends it over SSH.
    pub fn add_key(&self, id: &str, public_key: &str) -> anyhow::Result<()> {
        let key = public_key.trim();
        ensure!(
            !key.is_empty() && !key.contains(['\n', '\r']),
            "that isn't one public key"
        );
        let file = crate::cli::private_file("key.pub", key)?;
        self.cli.run(&[
            "ec2-instance-connect",
            "send-ssh-public-key",
            "--instance-id",
            id,
            "--instance-os-user",
            "ubuntu",
            "--ssh-public-key",
            &format!("file://{}", file.path().display()),
        ])?;
        Ok(())
    }

    /// Link's own keys gone from what AWS gives the box: nothing to do.
    /// Its key pair reached `authorized_keys` at the first boot alone, which
    /// `lnk box` edits itself, and a key pushed with Instance Connect lasts
    /// a minute.
    pub fn remove_keys(&self, _id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// Adds `tags` to it.
    pub fn tag(&self, id: &str, tags: &BTreeMap<String, String>) -> anyhow::Result<()> {
        for (key, value) in tags {
            tag_value(key)?;
            tag_value(value)?;
        }
        self.create_tags(id, tags)
    }

    fn create_tags(&self, id: &str, tags: &BTreeMap<String, String>) -> anyhow::Result<()> {
        let tags: Vec<_> = tags
            .iter()
            .map(|(k, v)| serde_json::json!({"Key": k, "Value": v}))
            .collect();
        self.cli.run(&[
            "ec2",
            "create-tags",
            "--resources",
            id,
            "--tags",
            &serde_json::to_string(&tags)?,
        ])?;
        Ok(())
    }

    /// Every box tagged `lnk-owner=<owner>` (with no owner, tagged
    /// `lnk-owner` at all) in `regions` (asked [`LIST_AT_ONCE`] at a
    /// time), or, with none,
    /// in every region the account has enabled and lets this login look
    /// in, and the regions an organization's policy denied it, skipped.
    pub fn list(
        &self,
        owner: Option<&str>,
        regions: &[String],
    ) -> anyhow::Result<(Vec<Found>, Vec<String>)> {
        if let Some(owner) = owner {
            tag_value(owner)?;
        }
        let every = regions.is_empty();
        let regions = match regions.is_empty() {
            false => regions.to_vec(),
            true => self.cli.json(&["ec2", "describe-regions"])?["Regions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| r["RegionName"].as_str().map(str::to_string))
                .collect(),
        };
        for region in &regions {
            tag_value(region)?;
        }
        // The regions asked at once, a few at a time: each is a start of
        // the AWS CLI and a round trip to the region.
        let filter = match owner {
            Some(owner) => format!("Name=tag:lnk-owner,Values={owner}"),
            None => "Name=tag-key,Values=lnk-owner".to_string(),
        };
        let mut answers = Vec::new();
        for some in regions.chunks(LIST_AT_ONCE) {
            answers.extend(std::thread::scope(|scope| {
                let asking: Vec<_> = some
                    .iter()
                    .map(|region| {
                        let filter = &filter;
                        scope.spawn(move || {
                            self.cli.json(&[
                                "ec2",
                                "describe-instances",
                                "--region",
                                region,
                                "--filters",
                                filter,
                                "Name=instance-state-name,Values=pending,running,stopping,stopped",
                            ])
                        })
                    })
                    .collect();
                asking
                    .into_iter()
                    .map(|asked| {
                        asked
                            .join()
                            .unwrap_or_else(|_| Err(anyhow::anyhow!("listing a region failed")))
                    })
                    .collect::<Vec<_>>()
            }));
        }
        let mut found = Vec::new();
        let mut denied = Vec::new();
        for (region, listed) in regions.iter().zip(answers) {
            let listed = match listed {
                Ok(listed) => listed,
                Err(err) if every && format!("{err:#}").contains("UnauthorizedOperation") => {
                    denied.push(region.clone());
                    continue;
                }
                Err(err) => return Err(err),
            };
            for instance in listed["Reservations"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|r| r["Instances"].as_array().into_iter().flatten())
            {
                found.push(found_of(instance, region));
            }
        }
        Ok((found, denied))
    }

    /// The SSH host keys its first boot printed on the console. The latest
    /// output where the instance type keeps it (Nitro), else what AWS
    /// kept, which comes a few minutes late.
    fn console_host_keys(&self, id: &str) -> anyhow::Result<Vec<String>> {
        let args = ["ec2", "get-console-output", "--instance-id", id];
        let output = match self.cli.json(&[&args[..], &["--latest"]].concat()) {
            Ok(output) => output,
            Err(_) => self.cli.json(&args)?,
        };
        Ok(link_cloud_contract::host_keys(
            output["Output"].as_str().unwrap_or_default(),
        ))
    }
}

/// Instance `id`'s public address, as `describe-instances` says it.
fn address_of(id: &str, instance: &serde_json::Value) -> anyhow::Result<String> {
    Ok(instance["PublicIpAddress"]
        .as_str()
        .within(format!(
            "{id} has no public address (a default VPC gives one)"
        ))?
        .to_string())
}

/// A box as `describe-instances` says it is, in `region`.
fn found_of(instance: &serde_json::Value, region: &str) -> Found {
    let tag = |key: &str| {
        instance["Tags"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| t["Key"] == key)
            .and_then(|t| t["Value"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    Found {
        tags: instance["Tags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| {
                Some((
                    t["Key"].as_str()?.to_string(),
                    t["Value"].as_str()?.to_string(),
                ))
            })
            .collect(),
        id: instance["InstanceId"].as_str().unwrap_or_default().into(),
        name: tag("lnk-box"),
        machine_id: tag("lnk-machine"),
        owner: link_cloud_contract::said(tag("lnk-owner")),
        deadline: tag("lnk-deadline").parse().ok(),
        place: region.to_string(),
        state: instance["State"]["Name"]
            .as_str()
            .unwrap_or("unknown")
            .into(),
        ip: instance["PublicIpAddress"].as_str().map(str::to_string),
        size: instance["InstanceType"].as_str().unwrap_or_default().into(),
        stopped_by_cloud: stopped_by_aws(instance),
        ..Found::default()
    }
}

/// Whether AWS stopped `instance` taking its spot machine back, by its
/// state reason (`Server.SpotInstanceShutdown`), rather than a person
/// (`Client.UserInitiatedShutdown`): none unless it's stopped.
pub fn stopped_by_aws(instance: &serde_json::Value) -> Option<bool> {
    let state = instance["State"]["Name"].as_str()?;
    if !matches!(state, "stopping" | "stopped") {
        return None;
    }
    Some(
        instance["StateReason"]["Code"]
            .as_str()
            .is_some_and(|code| code.starts_with("Server.SpotInstance")),
    )
}

/// An image's OS, by its name as AWS says it: Canonical's
/// (`ubuntu-noble-24.04-amd64-server-...`) or the Deep Learning AMI's
/// (`... (Ubuntu 24.04) ...`).
pub fn os_of_image(name: &str) -> Option<&'static str> {
    link_cloud_contract::OSES
        .iter()
        .find(|os| name.contains(os.trim_start_matches("ubuntu-")))
        .copied()
}

/// The key pair's name: Link's, and which key, without the key itself.
pub fn key_name(public_key: &str) -> String {
    format!("lnk-box-{}", crate::cli::short_hash(public_key.trim()))
}

/// An error unless `text` can be a tag's key or value Link sets (and so
/// can't change a filter or the CLI's shorthand): letters, digits and
/// dashes.
pub(crate) fn tag_value(text: &str) -> anyhow::Result<()> {
    ensure!(
        !text.is_empty()
            && text.len() <= 128
            && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "{:?} can't be a tag",
        link_format::clean(text)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aws_says_when_it_took_a_spot_machine_back() {
        let stopped = |code: &str| serde_json::json!({"State": {"Name": "stopped"}, "StateReason": {"Code": code}});
        assert_eq!(
            stopped_by_aws(&stopped("Server.SpotInstanceShutdown")),
            Some(true)
        );
        assert_eq!(
            stopped_by_aws(&stopped("Client.UserInitiatedShutdown")),
            Some(false)
        );
        let running = serde_json::json!({"State": {"Name": "running"}});
        assert_eq!(stopped_by_aws(&running), None);
    }
}
