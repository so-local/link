//! Attach existing resources and retain them when the VM is terminated.
use crate::cli::Aws;
use anyhow::ensure;
use link_cloud_contract::Attach;
use link_format::WithinExt;

impl Aws {
    /// AWS's network is a subnet ID. Its VPC has a preexisting lnk-box
    /// security group; provisioning firewall policy is the person's work.
    pub fn attachment_args(&self, attach: &Attach) -> anyhow::Result<Vec<String>> {
        attach.check()?;
        let mut args = Vec::new();
        let mut zone = None;
        if let Some(network) = &attach.network {
            let found = self
                .cli
                .json(&["ec2", "describe-subnets", "--subnet-ids", network])?;
            let subnet = &found["Subnets"][0];
            ensure!(
                subnet["SubnetId"].as_str() == Some(network),
                "subnet {network} is missing"
            );
            zone = subnet["AvailabilityZone"].as_str().map(str::to_string);
            let vpc = subnet["VpcId"].as_str().within("the subnet's VPC")?;
            let groups = self.cli.json(&[
                "ec2",
                "describe-security-groups",
                "--filters",
                "Name=group-name,Values=lnk-box",
                &format!("Name=vpc-id,Values={vpc}"),
            ])?;
            let group = groups["SecurityGroups"][0]["GroupId"]
                .as_str()
                .within("the subnet's existing lnk-box security group is missing")?;
            args.extend([
                "--subnet-id".into(),
                network.clone(),
                "--security-group-ids".into(),
                group.to_string(),
            ]);
        }
        if let Some(identity) = &attach.identity {
            let found = self.cli.json(&[
                "iam",
                "get-instance-profile",
                "--instance-profile-name",
                identity,
            ])?;
            ensure!(
                found["InstanceProfile"]["InstanceProfileName"].as_str() == Some(identity),
                "instance profile {identity} is missing"
            );
            args.extend(["--iam-instance-profile".into(), format!("Name={identity}")]);
        }
        if let Some(address) = &attach.address {
            let found =
                self.cli
                    .json(&["ec2", "describe-addresses", "--allocation-ids", address])?;
            ensure!(
                found["Addresses"][0]["AllocationId"].as_str() == Some(address),
                "address {address} is missing"
            );
        }
        for disk in attach.disks() {
            let zone = zone
                .as_deref()
                .within("attaching an AWS disk also requires its subnet in attach.network")?;
            let found = self
                .cli
                .json(&["ec2", "describe-volumes", "--volume-ids", disk])?;
            ensure!(
                found["Volumes"][0]["VolumeId"].as_str() == Some(disk),
                "disk {disk} is missing"
            );
            ensure!(
                found["Volumes"][0]["AvailabilityZone"].as_str() == Some(zone),
                "disk {disk} is in a different zone from the subnet"
            );
        }
        Ok(args)
    }

    pub fn attach_existing(&self, id: &str, attach: &Attach) -> anyhow::Result<()> {
        if attach.disks().is_empty() && attach.address.is_none() {
            return Ok(());
        }
        self.cli
            .run(&["ec2", "wait", "instance-running", "--instance-ids", id])?;
        for (index, disk) in attach.disks().iter().enumerate() {
            ensure!(index < 20, "at most 20 attached AWS disks");
            let device = format!("/dev/sd{}", char::from(b'f' + index as u8));
            self.cli.run(&[
                "ec2",
                "attach-volume",
                "--volume-id",
                disk,
                "--instance-id",
                id,
                "--device",
                &device,
            ])?;
            self.cli.run(&[
                "ec2",
                "modify-instance-attribute",
                "--instance-id",
                id,
                "--block-device-mappings",
                &serde_json::json!([{ "DeviceName":device, "Ebs":{"DeleteOnTermination":false}}])
                    .to_string(),
            ])?;
        }
        if let Some(address) = &attach.address {
            self.cli.run(&[
                "ec2",
                "associate-address",
                "--allocation-id",
                address,
                "--instance-id",
                id,
                "--no-allow-reassociation",
            ])?;
        }
        Ok(())
    }
}
