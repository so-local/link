//! The security groups Link's boxes share, per region: `lnk-box` lets
//! SSH in from anywhere, for a box reached at its port 22;
//! `lnk-box-tunnel` lets nothing in, for a box reached only through
//! Systems Manager, whose agent calls out over HTTPS. Each is made the
//! first time a box needs it and deleted with the last box.

use anyhow::{bail, ensure};
use link_format::WithinExt;

use crate::cli::Aws;

/// The group of boxes reached at their port 22.
const GROUP: &str = "lnk-box";
/// The group of boxes reached only through the tunnel.
const TUNNEL_GROUP: &str = "lnk-box-tunnel";

impl Aws {
    pub(super) fn default_vpc(&self) -> anyhow::Result<String> {
        let vpcs = self.cli.json(&[
            "ec2",
            "describe-vpcs",
            "--filters",
            "Name=isDefault,Values=true",
        ])?;
        match vpcs["Vpcs"][0]["VpcId"].as_str() {
            Some(vpc) => Ok(vpc.to_string()),
            None => bail!(
                "this AWS account has no default VPC in {}, which boxes go in \
                 (aws ec2 create-default-vpc makes one)",
                self.region
            ),
        }
    }

    fn find_group(&self, name: &str, vpc: &str) -> anyhow::Result<Option<String>> {
        let groups = self.cli.json(&[
            "ec2",
            "describe-security-groups",
            "--filters",
            &format!("Name=group-name,Values={name}"),
            &format!("Name=vpc-id,Values={vpc}"),
        ])?;
        Ok(groups["SecurityGroups"][0]["GroupId"]
            .as_str()
            .map(str::to_string))
    }

    /// The security group a box gets, made the first time: one letting
    /// SSH in, or, for a box reached only through the tunnel, one letting
    /// nothing in.
    pub(super) fn security_group(&self, tunnel: bool) -> anyhow::Result<String> {
        let name = if tunnel { TUNNEL_GROUP } else { GROUP };
        let vpc = self.default_vpc()?;
        if let Some(group) = self.find_group(name, &vpc)? {
            return Ok(group);
        }
        let made = match self.cli.json(&[
            "ec2",
            "create-security-group",
            "--group-name",
            name,
            "--description",
            if tunnel {
                "Link boxes: nothing in, SSH over Systems Manager (lnk box)"
            } else {
                "Link boxes: SSH in (lnk box)"
            },
            "--vpc-id",
            &vpc,
        ]) {
            Err(err) if made_meanwhile(&err) => {
                return self
                    .find_group(name, &vpc)?
                    .within("AWS said the security group exists, then didn't list it");
            }
            made => made?,
        };
        let group = made["GroupId"]
            .as_str()
            .within("AWS didn't say which security group it made")?
            .to_string();
        ensure!(!group.is_empty(), "no security group");
        if !tunnel {
            self.cli.run(&[
                "ec2",
                "authorize-security-group-ingress",
                "--group-id",
                &group,
                "--protocol",
                "tcp",
                "--port",
                "22",
                "--cidr",
                "0.0.0.0/0",
            ])?;
        }
        Ok(group)
    }

    /// Deletes both groups, where they are: once no box is left.
    pub(super) fn delete_groups(&self) -> anyhow::Result<()> {
        let vpc = self.default_vpc()?;
        for name in [GROUP, TUNNEL_GROUP] {
            if let Some(group) = self.find_group(name, &vpc)? {
                self.cli
                    .run(&["ec2", "delete-security-group", "--group-id", &group])?;
            }
        }
        Ok(())
    }
}

/// Whether making a group failed because another launch made it at the
/// same time: boxes made at once each find no group and make it, and all
/// but the first are told it's a duplicate, so each uses the one made.
fn made_meanwhile(err: &anyhow::Error) -> bool {
    err.to_string().contains("InvalidGroup.Duplicate")
}

#[cfg(test)]
mod tests {
    /// A group another launch made meanwhile is the one to use; any other
    /// failure isn't.
    #[test]
    fn a_group_made_meanwhile_is_used() {
        let duplicate = anyhow::anyhow!(
            "aws: [ERROR]: An error occurred (InvalidGroup.Duplicate) when calling the \
             CreateSecurityGroup operation: The security group 'lnk-box-tunnel' already \
             exists for VPC 'vpc-0ef4c6325982249bb'."
        );
        assert!(super::made_meanwhile(&duplicate));
        let denied = anyhow::anyhow!("An error occurred (UnauthorizedOperation)");
        assert!(!super::made_meanwhile(&denied));
    }
}
