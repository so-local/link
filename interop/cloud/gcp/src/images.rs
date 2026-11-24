//! Box images on Google Cloud: images of a stopped box's boot disk, made
//! with `gcloud`.
//!
//! An image is `lnk-<name>` in the account's project, labelled
//! `lnk-image=<name>` and `lnk-owner=<owner>`, and `lnk-hash=<hash>` when
//! it's saved with a hash. An image is the whole
//! project's, so a box from it can be in any zone. Only the project's
//! own images are ever used or deleted.

use link_format::WithinExt;
use std::collections::BTreeMap;

use anyhow::ensure;

use crate::boxes::{labels, last};
use crate::cli::Gcp;
use link_cloud_contract::FoundImage;

/// An error unless `id` can be an image's name in a project: lowercase
/// letters, digits and dashes, starting with a letter, at most 63.
pub fn check_id(id: &str) -> anyhow::Result<()> {
    let plain = (1..=63).contains(&id.len())
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    ensure!(plain, "{:?} isn't an image's name", link_format::clean(id));
    Ok(())
}

/// An image as `images describe` or `list` says it is.
pub fn found_image(image: &serde_json::Value) -> FoundImage {
    FoundImage {
        id: image["name"].as_str().unwrap_or_default().into(),
        name: image["labels"]["lnk-image"]
            .as_str()
            .unwrap_or_default()
            .into(),
        place: String::new(),
        state: match image["status"].as_str().unwrap_or("unknown") {
            "READY" => "ready".into(),
            "PENDING" => "pending".into(),
            "FAILED" => "failed".into(),
            other => other.to_lowercase(),
        },
        disk_gb: image["diskSizeGb"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .or(image["diskSizeGb"].as_u64()),
        arch: match image["architecture"].as_str() {
            Some("X86_64") => Some("x86_64".into()),
            Some("ARM64") => Some("arm64".into()),
            _ => None,
        },
        created: image["creationTimestamp"]
            .as_str()
            .map(str::to_string)
            .filter(|c| !c.is_empty()),
        hash: image["labels"][link_cloud_contract::IMAGE_HASH]
            .as_str()
            .map(str::to_string)
            .filter(|h| !h.is_empty()),
    }
}

impl Gcp {
    /// Image `id` of the project, as Google says it is; none when it's
    /// gone.
    fn own_image(&self, id: &str) -> anyhow::Result<Option<serde_json::Value>> {
        check_id(id)?;
        match self
            .cli
            .json(&["compute", "images", "describe", id, "--format", "json"])
        {
            Ok(image) => Ok(Some(image)),
            Err(err) if err.to_string().contains("was not found") => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// The image to make a box from: one of the project's own, ready.
    pub fn launch_image(&self, id: &str) -> anyhow::Result<String> {
        let image = self
            .own_image(id)?
            .within(format!("no image {id} in this project"))?;
        let found = found_image(&image);
        ensure!(
            found.state == "ready",
            "image {id} is {}, not ready",
            link_format::clean(&found.state)
        );
        Ok(id.to_string())
    }

    /// Saves instance `id`'s boot disk as image `lnk-<name>`, labelled
    /// with `tags`. The instance is stopped first by the caller: Google
    /// refuses a disk in use. `images create` returns once it's ready.
    pub fn save_image(
        &self,
        id: &str,
        name: &str,
        tags: &BTreeMap<String, String>,
    ) -> anyhow::Result<FoundImage> {
        link_cloud_contract::check_image(name)?;
        let labels = labels(tags)?;
        let instance = self.cli.json(&[
            "compute",
            "instances",
            "describe",
            id,
            "--zone",
            &self.zone,
            "--format",
            "json",
        ])?;
        let disk = instance["disks"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["boot"] == true)
            .map(|d| last(&d["source"]))
            .filter(|d| check_id(d).is_ok())
            .within(format!("{id} has no boot disk Google names"))?;
        let image = link_cloud_contract::image_name(name);
        self.cli.run(&[
            "compute",
            "images",
            "create",
            &image,
            "--source-disk",
            &disk,
            "--source-disk-zone",
            &self.zone,
            "--labels",
            &labels,
            "--description",
            &format!("lnk box image create: {name}"),
            "--format",
            "json",
        ])?;
        let saved = self
            .own_image(&image)?
            .within(format!("Google doesn't list image {image} it made"))?;
        Ok(found_image(&saved))
    }

    /// The project's own images; with `owner`, only its Link-labelled images.
    pub fn images(&self, owner: Option<&str>) -> anyhow::Result<Vec<FoundImage>> {
        if let Some(owner) = owner {
            crate::boxes::label_value(owner)?;
        }
        let filter = owner.map(|owner| format!("labels.lnk-owner={owner}"));
        let mut args = vec!["compute", "images", "list", "--no-standard-images"];
        if let Some(filter) = &filter {
            args.extend(["--filter", filter]);
        }
        args.extend(["--format", "json"]);
        let found = self.cli.json(&args)?;
        let mut images: Vec<FoundImage> = found
            .as_array()
            .into_iter()
            .flatten()
            .map(found_image)
            .filter(|i| owner.is_none() || !i.name.is_empty())
            .collect();
        images.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(images)
    }

    /// Deletes image `id`, if it's one Link made for `owner` (labelled
    /// so); gone already is done. An image on Google Cloud holds its own
    /// copy of the disk: there's no snapshot besides.
    pub fn remove_image(&self, id: &str, owner: &str) -> anyhow::Result<()> {
        let Some(image) = self.own_image(id)? else {
            return Ok(());
        };
        ensure!(
            image["labels"]["lnk-image"].is_string() && image["labels"]["lnk-owner"] == owner,
            "image {id} isn't one Link made for you: it's left as it is"
        );
        match self
            .cli
            .run(&["compute", "images", "delete", id, "--quiet"])
        {
            Err(err) if err.to_string().contains("was not found") => Ok(()),
            other => other.map(|_| ()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_is_read_in_links_words() {
        let image = serde_json::json!({
            "name": "lnk-web-ready", "status": "READY", "diskSizeGb": "30",
            "architecture": "X86_64", "creationTimestamp": "2026-10-01T03:00:00.000-07:00",
            "labels": {"lnk-image": "web-ready", "lnk-owner": "github-7", "lnk-hash": "3f2a9c"}
        });
        assert_eq!(
            found_image(&image),
            FoundImage {
                id: "lnk-web-ready".into(),
                name: "web-ready".into(),
                place: String::new(),
                state: "ready".into(),
                disk_gb: Some(30),
                arch: Some("x86_64".into()),
                created: Some("2026-10-01T03:00:00.000-07:00".into()),
                hash: Some("3f2a9c".into()),
            }
        );
        for bad in ["", "Lnk", "-x", "lnk_web", "a/b", &"a".repeat(64)] {
            assert!(check_id(bad).is_err(), "{bad}");
        }
    }
}
