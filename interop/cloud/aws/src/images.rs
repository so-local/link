//! Box images on AWS: AMIs of a stopped box's disk, made with the `aws`
//! CLI.
//!
//! An image is an AMI named `lnk-<name>`, its snapshot with it, both
//! tagged `lnk-image=<name>` and `lnk-owner=<owner>`, and `lnk-hash=<hash>`
//! when it's saved with a hash. AMIs live in one
//! region: a box from one is made there. Only the account's own AMIs
//! (`--owners self`) are ever used or deleted, never a public one.

use link_format::WithinExt;
use std::collections::BTreeMap;

use anyhow::{bail, ensure};

use crate::cli::Aws;
use link_cloud_contract::FoundImage;
use link_format::{Report, aside, sentence};

/// How many regions [`Aws::images`] asks at once.
const AT_ONCE: usize = 8;
/// How often saving asks whether the image is ready (the CLI's own `aws
/// ec2 wait` asks every 15 s; each ask here is a start of the CLI), and
/// how long it waits before giving up: an hour, for a big disk.
const POLL: std::time::Duration = std::time::Duration::from_secs(10);
const WAIT: std::time::Duration = std::time::Duration::from_secs(3600);

/// An error unless `id` is an AMI's id, as AWS gives them.
pub fn check_id(id: &str) -> anyhow::Result<()> {
    let plain = id
        .strip_prefix("ami-")
        .is_some_and(|hex| !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()));
    ensure!(plain, "{:?} isn't an AMI's id", link_format::clean(id));
    Ok(())
}

/// `--tag-specifications` for a new image and its snapshot.
pub fn image_tags(tags: &BTreeMap<String, String>) -> anyhow::Result<String> {
    for (key, value) in tags {
        crate::boxes::tag_value(key)?;
        crate::boxes::tag_value(value)?;
    }
    let tags: Vec<_> = tags
        .iter()
        .map(|(k, v)| serde_json::json!({"Key": k, "Value": v}))
        .collect();
    let specs: Vec<_> = ["image", "snapshot"]
        .into_iter()
        .map(|kind| serde_json::json!({"ResourceType": kind, "Tags": tags}))
        .collect();
    Ok(serde_json::to_string(&specs)?)
}

/// An image as `describe-images` says it is, in `region`.
pub fn found_image(image: &serde_json::Value, region: &str) -> FoundImage {
    let tag = |key: &str| {
        image["Tags"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| t["Key"] == key)
            .and_then(|t| t["Value"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let disk_gb = root(image).and_then(|m| m["Ebs"]["VolumeSize"].as_u64());
    FoundImage {
        id: image["ImageId"].as_str().unwrap_or_default().into(),
        name: tag("lnk-image"),
        place: region.to_string(),
        state: match image["State"].as_str().unwrap_or("unknown") {
            "available" => "ready".into(),
            "pending" => "pending".into(),
            "failed" | "error" | "invalid" => "failed".into(),
            other => other.to_string(),
        },
        disk_gb,
        arch: image["Architecture"].as_str().map(str::to_string),
        created: image["CreationDate"]
            .as_str()
            .map(str::to_string)
            .filter(|c| !c.is_empty()),
        hash: link_cloud_contract::said(tag(link_cloud_contract::IMAGE_HASH)),
    }
}

/// The image's root disk, among its block device mappings.
fn root(image: &serde_json::Value) -> Option<&serde_json::Value> {
    image["BlockDeviceMappings"]
        .as_array()?
        .iter()
        .find(|m| m["DeviceName"] == image["RootDeviceName"])
}

impl Aws {
    /// The account's own image `id` here, as `describe-images` says it
    /// is; none when it's gone.
    fn own_image(&self, id: &str) -> anyhow::Result<Option<serde_json::Value>> {
        check_id(id)?;
        let found = match self.cli.json(&[
            "ec2",
            "describe-images",
            "--owners",
            "self",
            "--image-ids",
            id,
        ]) {
            Ok(found) => found,
            Err(err) if err.to_string().contains("InvalidAMIID") => return Ok(None),
            Err(err) => return Err(err),
        };
        Ok(found["Images"].as_array().and_then(|i| i.first()).cloned())
    }

    /// The image to make a box from, and its root disk's device name: one
    /// of the account's own, ready.
    pub fn launch_image(&self, id: &str) -> anyhow::Result<(String, String)> {
        let image = self
            .own_image(id)?
            .within(format!("no image {id} of this account in {}", self.region))?;
        let found = found_image(&image, &self.region);
        ensure!(
            found.state == "ready",
            "image {id} is {}, not ready",
            link_format::clean(&found.state)
        );
        let device = image["RootDeviceName"]
            .as_str()
            .unwrap_or("/dev/sda1")
            .to_string();
        Ok((id.to_string(), device))
    }

    /// Saves instance `id`'s disks as image `lnk-<name>`, tagged with
    /// `tags`, and waits until it's ready. The instance is stopped first
    /// by the caller; were it running, AWS would reboot it to save it
    /// whole.
    #[expect(
        clippy::disallowed_methods,
        reason = "polls AWS for an image's state, which it doesn't push"
    )]
    pub fn save_image(
        &self,
        id: &str,
        name: &str,
        tags: &BTreeMap<String, String>,
    ) -> anyhow::Result<FoundImage> {
        link_cloud_contract::check_image(name)?;
        let left_out = self.attached_devices(id)?;
        let left_out = left_out_mappings(&left_out);
        let mut args = vec![
            "ec2".to_string(),
            "create-image".into(),
            "--instance-id".into(),
            id.into(),
            "--name".into(),
            link_cloud_contract::image_name(name),
            "--description".into(),
            format!("lnk box image create: {name}"),
            "--tag-specifications".into(),
            image_tags(tags)?,
        ];
        if let Some(left_out) = left_out {
            args.extend(["--block-device-mappings".into(), left_out]);
        }
        let made = self
            .cli
            .json(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
        let image = made["ImageId"]
            .as_str()
            .within("AWS didn't say which image it made")?
            .to_string();
        check_id(&image)?;
        let start = std::time::Instant::now();
        loop {
            // Just made, AWS may not list it yet.
            let saved = self
                .own_image(&image)?
                .map(|i| found_image(&i, &self.region));
            let state = saved
                .as_ref()
                .map_or_else(|| "pending".to_string(), |i| i.state.clone());
            match saved {
                Some(saved) if state == "ready" => return Ok(saved),
                _ if state != "pending" => {
                    bail!(
                        "image {image} isn't ready: it's {}",
                        link_format::clean(&state)
                    )
                }
                _ if start.elapsed() >= WAIT => bail!(
                    "image {image} isn't ready after {} minutes",
                    WAIT.as_secs() / 60
                ),
                _ => std::thread::sleep(POLL),
            }
        }
    }

    /// Instance `id`'s devices but its root: the volumes attached to it,
    /// which an image leaves out.
    fn attached_devices(&self, id: &str) -> anyhow::Result<Vec<String>> {
        let found = self
            .cli
            .json(&["ec2", "describe-instances", "--instance-ids", id])?;
        let instance = &found["Reservations"][0]["Instances"][0];
        let root = instance["RootDeviceName"]
            .as_str()
            .within("AWS didn't say the instance's root device")?;
        Ok(instance["BlockDeviceMappings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["DeviceName"].as_str())
            .filter(|device| *device != root)
            .map(str::to_string)
            .collect())
    }

    /// The account's own images; with `owner`, only its Link-tagged images. In
    /// `regions` (asked [`AT_ONCE`] at a time), or, with none, in every
    /// region the account has enabled and lets this login look in.
    pub fn images(
        &self,
        owner: Option<&str>,
        regions: &[String],
    ) -> anyhow::Result<Vec<FoundImage>> {
        if let Some(owner) = owner {
            crate::boxes::tag_value(owner)?;
        }
        let every = regions.is_empty();
        let regions = match every {
            false => regions.to_vec(),
            true => self.cli.json(&["ec2", "describe-regions"])?["Regions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| r["RegionName"].as_str().map(str::to_string))
                .collect(),
        };
        for region in &regions {
            crate::boxes::tag_value(region)?;
        }
        let filter = owner.map(|owner| format!("Name=tag:lnk-owner,Values={owner}"));
        let mut answers = Vec::new();
        for some in regions.chunks(AT_ONCE) {
            answers.extend(std::thread::scope(|scope| {
                let asking: Vec<_> = some
                    .iter()
                    .map(|region| {
                        let filter = &filter;
                        scope.spawn(move || {
                            let mut args = vec![
                                "ec2",
                                "describe-images",
                                "--region",
                                region,
                                "--owners",
                                "self",
                            ];
                            if let Some(filter) = filter {
                                args.extend(["--filters", filter, "Name=tag-key,Values=lnk-image"]);
                            }
                            self.cli.json(&args)
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
                    denied.push(region.as_str());
                    continue;
                }
                Err(err) => return Err(err),
            };
            for image in listed["Images"].as_array().into_iter().flatten() {
                found.push(found_image(image, region));
            }
        }
        if !denied.is_empty() {
            aside(Report::warning(sentence!(
                "Not looking for images in {}: this AWS login isn't allowed to look there.",
                denied.join(", ")
            )));
        }
        found.sort_by(|a, b| a.name.cmp(&b.name).then(a.place.cmp(&b.place)));
        Ok(found)
    }

    /// Deregisters image `id` and deletes its snapshots, if it's one Link
    /// made for `owner` (tagged so); gone already is done.
    pub fn remove_image(&self, id: &str, owner: &str) -> anyhow::Result<()> {
        let Some(image) = self.own_image(id)? else {
            return Ok(());
        };
        let found = found_image(&image, &self.region);
        let tagged =
            |key: &str, value: Option<&str>| {
                image["Tags"].as_array().into_iter().flatten().any(|t| {
                    t["Key"] == key && value.is_none_or(|v| t["Value"].as_str() == Some(v))
                })
            };
        ensure!(
            !found.name.is_empty() && tagged("lnk-owner", Some(owner)),
            "image {id} isn't one Link made for you: it's left as it is"
        );
        let snapshots: Vec<String> = image["BlockDeviceMappings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["Ebs"]["SnapshotId"].as_str())
            .filter(|s| {
                s.strip_prefix("snap-")
                    .is_some_and(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
            })
            .map(str::to_string)
            .collect();
        self.cli
            .run(&["ec2", "deregister-image", "--image-id", id])?;
        for snapshot in snapshots {
            match self
                .cli
                .run(&["ec2", "delete-snapshot", "--snapshot-id", &snapshot])
            {
                Err(err) if err.to_string().contains("InvalidSnapshot.NotFound") => {}
                other => {
                    other.within(format!("deleting {id}'s snapshot {snapshot}"))?;
                }
            }
        }
        Ok(())
    }
}

/// `--block-device-mappings` leaving each of `devices` out of an image
/// (`NoDevice`), so it holds its root disk alone; none for none.
fn left_out_mappings(devices: &[String]) -> Option<String> {
    (!devices.is_empty()).then(|| {
        serde_json::Value::Array(
            devices
                .iter()
                .map(|device| serde_json::json!({"DeviceName": device, "NoDevice": ""}))
                .collect(),
        )
        .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_is_read_in_links_words() {
        let image = serde_json::json!({
            "ImageId": "ami-0abc", "State": "available", "Architecture": "x86_64",
            "CreationDate": "2026-10-01T10:00:00.000Z", "RootDeviceName": "/dev/sda1",
            "BlockDeviceMappings": [
                {"DeviceName": "/dev/sdb", "Ebs": {"VolumeSize": 100}},
                {"DeviceName": "/dev/sda1", "Ebs": {"VolumeSize": 30, "SnapshotId": "snap-1"}}
            ],
            "Tags": [{"Key": "lnk-image", "Value": "web-ready"}, {"Key": "lnk-hash", "Value": "3f2a9c"}]
        });
        assert_eq!(
            found_image(&image, "eu-west-2"),
            FoundImage {
                id: "ami-0abc".into(),
                name: "web-ready".into(),
                place: "eu-west-2".into(),
                state: "ready".into(),
                disk_gb: Some(30),
                arch: Some("x86_64".into()),
                created: Some("2026-10-01T10:00:00.000Z".into()),
                hash: Some("3f2a9c".into()),
            }
        );
        assert!(check_id("ami-0abc123").is_ok());
        for bad in ["ami-", "ami-xyz", "snap-1", "--image-id=x", "ami-1 x"] {
            assert!(check_id(bad).is_err(), "{bad}");
        }
        let specs: serde_json::Value = serde_json::from_str(
            &image_tags(&link_cloud_contract::image_tags("web-ready", "github-7")).unwrap(),
        )
        .unwrap();
        assert_eq!(specs[0]["ResourceType"], "image");
        assert_eq!(specs[1]["ResourceType"], "snapshot");
        assert_eq!(specs[1]["Tags"][1]["Value"], "github-7");
        // A hash is one more tag, on the image and its snapshot.
        let mut hashed = link_cloud_contract::image_tags("web-ready", "github-7");
        hashed.insert(link_cloud_contract::IMAGE_HASH.into(), "3f2a9c".into());
        let specs: serde_json::Value = serde_json::from_str(&image_tags(&hashed).unwrap()).unwrap();
        assert!(
            specs[1]["Tags"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!({"Key": "lnk-hash", "Value": "3f2a9c"}))
        );
    }
}
