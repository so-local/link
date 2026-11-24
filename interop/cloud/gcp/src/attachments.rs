//! Preexisting resources: lookup and attachment only, never creation or deletion.
use crate::cli::Gcp;
use anyhow::ensure;
use link_cloud_contract::Attach;
use link_format::WithinExt;

impl Gcp {
    pub fn attachment_args(&self, attach: &Attach) -> anyhow::Result<Vec<String>> {
        attach.check()?;
        let mut args = Vec::new();
        if let Some(network) = &attach.network {
            let found =
                self.cli
                    .json(&["compute", "networks", "describe", network, "--format=json"])?;
            ensure!(
                found["name"].as_str() == Some(network),
                "network {network} is missing"
            );
            args.extend(["--network".into(), network.clone()]);
        }
        if let Some(identity) = &attach.identity {
            let project = &self.project;
            let identity = if identity.contains('@') {
                identity.clone()
            } else {
                format!("{identity}@{project}.iam.gserviceaccount.com")
            };
            let found = self.cli.json(&[
                "iam",
                "service-accounts",
                "describe",
                &identity,
                "--format=json",
            ])?;
            ensure!(
                found["email"].as_str() == Some(&identity),
                "identity {identity} is missing"
            );
            args.extend([
                "--service-account".into(),
                identity,
                "--scopes=cloud-platform".into(),
            ]);
        }
        if let Some(address) = &attach.address {
            let region = self.zone.rsplit_once('-').within("a zone has a region")?.0;
            let found = self.cli.json(&[
                "compute",
                "addresses",
                "describe",
                address,
                "--region",
                region,
                "--format=json",
            ])?;
            ensure!(
                found["name"].as_str() == Some(address),
                "address {address} is missing"
            );
            args.extend(["--address".into(), address.clone()]);
        }
        for disk in attach.disks() {
            let found = self.cli.json(&[
                "compute",
                "disks",
                "describe",
                disk,
                "--zone",
                &self.zone,
                "--format=json",
            ])?;
            ensure!(
                found["name"].as_str() == Some(disk),
                "disk {disk} is missing"
            );
            args.extend([
                "--disk".into(),
                format!("name={disk},boot=no,auto-delete=no,mode=rw"),
            ]);
        }
        Ok(args)
    }
}
