//! What a region's boxes share: the security group Link makes for them
//! (and the key pair, for an older `lnk box`), deleted with the last box,
//! and their spot requests.

use super::*;
use link_format::{aside, sentence};

impl Aws {
    /// Deletes the security group and Link's key pairs, unless a Link
    /// box in this region still uses them (asked of AWS, not remembered).
    pub fn cleanup(&self) -> anyhow::Result<()> {
        // A request left open is said; the group and key still go.
        if let Err(err) = self.cleanup_spot_requests() {
            aside(link_cloud_contract::warned(
                sentence!(
                    "Link's spot requests here weren't all cancelled, and aws ec2 \
                     describe-spot-instance-requests --filters Name=tag-key,Values=lnk-box lists \
                     them."
                ),
                err,
            ));
        }
        let left = self.cli.json(&[
            "ec2",
            "describe-instances",
            "--filters",
            "Name=tag-key,Values=lnk-box",
            "Name=instance-state-name,Values=pending,running,stopping,stopped,shutting-down",
        ])?;
        if left["Reservations"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
        {
            return Ok(());
        }
        self.delete_groups()?;
        // Each computer's, whichever made a box here before certificates.
        let pairs = self.cli.json(&[
            "ec2",
            "describe-key-pairs",
            "--filters",
            "Name=key-name,Values=lnk-box-*",
        ])?;
        for name in pairs["KeyPairs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["KeyName"].as_str())
            .filter(|name| name.starts_with("lnk-box-"))
        {
            self.cli
                .run(&["ec2", "delete-key-pair", "--key-name", name])?;
        }
        Ok(())
    }

    pub(super) fn cancel_spot_requests(&self, requests: &[String]) -> anyhow::Result<()> {
        if requests.is_empty() {
            return Ok(());
        }
        let mut args = vec!["ec2", "cancel-spot-instance-requests"];
        args.push("--spot-instance-request-ids");
        args.extend(requests.iter().map(String::as_str));
        self.cli.run(&args)?;
        Ok(())
    }

    /// Cancels Link's spot requests here (tagged `lnk-box`) whose box is
    /// gone, or which have none: left open, one would make a box again.
    fn cleanup_spot_requests(&self) -> anyhow::Result<()> {
        let found = self.cli.json(&[
            "ec2",
            "describe-spot-instance-requests",
            "--filters",
            "Name=tag-key,Values=lnk-box",
            "Name=state,Values=open,active,disabled",
        ])?;
        let requests: Vec<(String, Option<String>)> = found["SpotInstanceRequests"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let id = r["SpotInstanceRequestId"].as_str()?;
                Some((id.to_string(), r["InstanceId"].as_str().map(str::to_string)))
            })
            .collect();
        let mut gone = Vec::new();
        for (request, instance) in requests {
            let alive = match &instance {
                None => false,
                Some(instance) => !matches!(
                    self.state(instance)?.as_str(),
                    "gone" | "terminated" | "shutting-down"
                ),
            };
            if !alive {
                gone.push(request);
            }
        }
        self.cancel_spot_requests(&gone)
    }

    /// Link's key pair in this region, imported if it isn't there.
    pub(super) fn key_pair(&self, public_key: &str) -> anyhow::Result<String> {
        let name = key_name(public_key);
        let found = self
            .cli
            .run(&["ec2", "describe-key-pairs", "--key-names", &name]);
        if found.is_err() {
            let key = crate::cli::private_file("key.pub", public_key)?;
            imported_once(self.cli.run(&[
                "ec2",
                "import-key-pair",
                "--key-name",
                &name,
                "--public-key-material",
                &format!("fileb://{}", key.path().display()),
            ]))?;
        }
        Ok(name)
    }
}

/// The key pair's import, by this launch or by another at the same time:
/// boxes launched at once each find no key and import it, and all but
/// the first are told it's a duplicate. Its name is the key's own hash
/// ([`key_name`]), so the key under that name is this one.
fn imported_once(imported: anyhow::Result<Vec<u8>>) -> anyhow::Result<()> {
    match imported {
        Err(err) if err.to_string().contains("InvalidKeyPair.Duplicate") => Ok(()),
        other => other.map(drop),
    }
}

#[cfg(test)]
mod tests {
    /// A key another launch imported meanwhile is imported; any other
    /// failure isn't.
    #[test]
    fn a_key_imported_meanwhile_is_imported() {
        let duplicate = anyhow::anyhow!(
            "aws: [ERROR]: An error occurred (InvalidKeyPair.Duplicate) when calling the \
             ImportKeyPair operation: The keypair already exists"
        );
        assert!(super::imported_once(Err(duplicate)).is_ok());
        assert!(super::imported_once(Ok(Vec::new())).is_ok());
        let denied = anyhow::anyhow!("An error occurred (UnauthorizedOperation)");
        assert!(super::imported_once(Err(denied)).is_err());
    }
}
