//! `lnk-aws`'s box images with a fake `aws`: an AMI saved from a box,
//! found by its tags, a box made from it, and the AMI deleted with its
//! snapshot. Only the account's own, tagged as Link's, are touched.

use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use link_test_kit::TempDir;

use crate::common;

const IMAGE: &str = r#"{"Images": [{"ImageId": "ami-0a1", "State": "available", "Architecture": "x86_64", "CreationDate": "2026-10-01T10:00:00.000Z", "RootDeviceName": "/dev/sda1", "BlockDeviceMappings": [{"DeviceName": "/dev/sda1", "Ebs": {"VolumeSize": 30, "SnapshotId": "snap-0b2"}}], "Tags": [{"Key": "lnk-image", "Value": "web-ready"}, {"Key": "lnk-hash", "Value": "3f2a9c"}, {"Key": "lnk-owner", "Value": "github-7"}]}]}"#;

/// Answers each call the way EC2 would.
fn ec2() -> String {
    format!(
        r#"case "$*" in
  "ec2 create-image "*) echo '{{"ImageId": "ami-0a1"}}' ;;
  "ec2 describe-instances --instance-ids i-1") echo '{{"Reservations": [{{"Instances": [{{"RootDeviceName": "/dev/sda1", "BlockDeviceMappings": [{{"DeviceName": "/dev/sda1"}}, {{"DeviceName": "/dev/sdf"}}]}}]}}]}}' ;;
  "ec2 describe-images --owners self --image-ids ami-0a1") echo '{IMAGE}' ;;
  "ec2 describe-images --owners self --image-ids ami-0e1") echo '{{"Images": [{{"ImageId": "ami-0e1", "State": "available", "Tags": [{{"Key": "Name", "Value": "mine"}}]}}]}}' ;;
  "ec2 describe-images --owners self --image-ids "*) echo 'An error occurred (InvalidAMIID.NotFound)' >&2; exit 254 ;;
  "ec2 describe-regions") echo '{{"Regions": [{{"RegionName": "eu-west-2"}}, {{"RegionName": "us-east-1"}}, {{"RegionName": "ap-south-1"}}]}}' ;;
  "ec2 describe-images --region eu-west-2 "*) echo '{IMAGE}' ;;
  "ec2 describe-images --region ap-south-1 "*) echo 'An error occurred (UnauthorizedOperation)' >&2; exit 254 ;;
  "ec2 describe-images --region "*) echo '{{"Images": []}}' ;;
  "ec2 deregister-image "*|"ec2 delete-snapshot "*) echo '{{}}' ;;
  "ec2 describe-key-pairs "*) echo '{{}}' ;;
  "ec2 describe-vpcs "*) echo '{{"Vpcs": [{{"VpcId": "vpc-1"}}]}}' ;;
  "ec2 describe-security-groups "*) echo '{{"SecurityGroups": [{{"GroupId": "sg-1"}}]}}' ;;
  "ec2 run-instances "*) echo '{{"Instances": [{{"InstanceId": "i-2"}}]}}' ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac"#
    )
}

/// `lnk-aws <command>` with `request`: its reply, or its error.
fn call(
    dir: &Path,
    command: &str,
    request: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut child = common::lnk_aws(dir, &dir.join("aws"))
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    match output.status.success() {
        true => Ok(serde_json::from_slice(&output.stdout).unwrap()),
        false => Err(String::from_utf8_lossy(&output.stderr).into_owned()),
    }
}

fn account() -> serde_json::Value {
    serde_json::json!({"name": "aws", "kind": "aws", "place": "eu-west-2", "region": "eu-west-2", "env": {}})
}

#[test]
fn image_inventory_without_owner_preserves_all_account_native_ids() {
    let dir = TempDir::new("images-all-owners");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    common::fake(
        &dir,
        r#"case "$*" in
      "ec2 describe-images --region eu-west-2 --owners self")
        echo '{"Images":[{"ImageId":"ami-before","State":"available","Tags":[]},{"ImageId":"ami-foreign","State":"available","Tags":[{"Key":"lnk-owner","Value":"github-other"},{"Key":"lnk-image","Value":"other"}]}]}' ;;
      *) echo "unexpected: $*" >&2;exit 1 ;;
    esac"#,
    );
    let reply = call(
        &dir,
        "image-list",
        serde_json::json!({"account":account(),"places":["eu-west-2"]}),
    )
    .unwrap();
    assert_eq!(reply["images"].as_array().unwrap().len(), 2);
    assert_eq!(reply["images"][0]["id"], "ami-before");
    assert_eq!(reply["images"][1]["id"], "ami-foreign");
    assert!(!log(&dir).contains("--filters"));
}

fn log(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("aws.log")).unwrap_or_default()
}

#[test]
fn a_box_is_saved_as_an_ami_and_its_snapshot_both_tagged() {
    let dir = TempDir::new("images-save");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    common::fake(&dir, &ec2());
    let saved = call(
        &dir,
        "image-save",
        serde_json::json!({"account": account(), "place": "eu-west-2", "id": "i-1",
            "image": "web-ready", "tags": {"lnk-image": "web-ready", "lnk-hash": "3f2a9c", "lnk-owner": "github-7"}}),
    )
    .unwrap();
    assert_eq!(
        saved["images"][0],
        serde_json::json!({"id": "ami-0a1", "name": "web-ready", "place": "eu-west-2",
            "state": "ready", "disk_gb": 30, "arch": "x86_64",
            "created": "2026-10-01T10:00:00.000Z", "hash": "3f2a9c"})
    );
    let log = log(&dir);
    let create = log
        .lines()
        .find(|l| l.starts_with("ec2 create-image "))
        .unwrap();
    assert!(
        create.starts_with("ec2 create-image --instance-id i-1 --name lnk-web-ready "),
        "{create}"
    );
    // The box is stopped first: AWS may reboot it, never save it half-written.
    assert!(!create.contains("--no-reboot"), "{create}");
    // Its root disk alone: a volume attached to the box stays out of the
    // image, so no box made from it holds or keeps a copy.
    assert!(
        create.contains(r#"--block-device-mappings [{"DeviceName":"/dev/sdf","NoDevice":""}]"#),
        "{create}"
    );
    assert!(!create.contains(r#""DeviceName":"/dev/sda1""#), "{create}");
    for part in [
        r#"{"ResourceType":"image","Tags":[{"Key":"lnk-hash","Value":"3f2a9c"},{"Key":"lnk-image","Value":"web-ready"},{"Key":"lnk-owner","Value":"github-7"}]}"#,
        r#"{"ResourceType":"snapshot","Tags":[{"Key":"lnk-hash","Value":"3f2a9c"},{"Key":"lnk-image""#,
    ] {
        assert!(create.contains(part), "{part} in {create}");
    }
    // Ready once AWS says so, asked of it directly.
    assert!(log.contains("ec2 describe-images --owners self --image-ids ami-0a1"));
    assert!(!log.contains("ec2 wait"), "{log}");

    // A name AWS's CLI could read as something else never reaches it.
    let bad = call(
        &dir,
        "image-save",
        serde_json::json!({"account": account(), "id": "i-1", "image": "--no-reboot",
            "tags": {"lnk-image": "x", "lnk-owner": "github-7"}}),
    );
    assert!(bad.unwrap_err().contains("can't name an image"));
    assert_eq!(
        self::log(&dir).matches("ec2 create-image").count(),
        1,
        "one image made"
    );
}

#[test]
fn images_are_found_by_their_tags_in_every_region() {
    let dir = TempDir::new("images-list");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    common::fake(&dir, &ec2());
    let error = call(
        &dir,
        "image-list",
        serde_json::json!({"account": account(), "owner": "github-7"}),
    );
    let listed = error.unwrap();
    assert_eq!(listed["images"].as_array().unwrap().len(), 1);
    assert_eq!(listed["images"][0]["place"], "eu-west-2");
    let log = log(&dir);
    assert!(
        log.contains(
            "ec2 describe-images --region us-east-1 --owners self --filters Name=tag:lnk-owner,Values=github-7 Name=tag-key,Values=lnk-image"
        ),
        "{log}"
    );
    // An owner that isn't a tag never reaches the filter.
    let bad = call(
        &dir,
        "image-list",
        serde_json::json!({"account": account(), "owner": "x,Name=tag-key"}),
    );
    assert!(bad.is_err());
}

#[test]
fn a_box_from_an_image_is_the_accounts_own_ami() {
    let dir = TempDir::new("images-launch");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    common::fake(&dir, &ec2());
    let spec = serde_json::json!({"name": "web-1", "id": "k3v9q2x7m1ab", "size": "t3.small",
        "user_data": "#cloud-config\n", "public_key": "ssh-ed25519 AAAAC3Nza lnk-box",
        "disk_gb": 30});
    let made = call(
        &dir,
        "launch",
        serde_json::json!({"account": account(), "place": "eu-west-2", "spec": spec,
            "image": "ami-0a1"}),
    )
    .unwrap();
    assert_eq!(made["id"], "i-2");
    let log = log(&dir);
    let run = log
        .lines()
        .find(|l| l.starts_with("ec2 run-instances "))
        .unwrap();
    assert!(run.contains("--image-id ami-0a1 "), "{run}");
    assert_eq!(
        common::json_argument(run, "--block-device-mappings"),
        serde_json::json!([{"DeviceName": "/dev/sda1", "Ebs": {"Encrypted": true, "DeleteOnTermination": true, "VolumeSize": 30, "VolumeType": "gp3"}}])
    );
    // Not Canonical's image: no SSM parameter asked.
    assert!(!log.contains("ssm get-parameter"), "{log}");
    // Only one of the account's own: a public AMI, or one gone, is refused.
    let public = call(
        &dir,
        "launch",
        serde_json::json!({"account": account(), "spec": spec, "image": "ami-0ffffff"}),
    );
    assert!(
        public
            .unwrap_err()
            .contains("no image ami-0ffffff of this account")
    );
    let odd = call(
        &dir,
        "launch",
        serde_json::json!({"account": account(), "spec": spec, "image": "ubuntu"}),
    );
    assert!(odd.unwrap_err().contains("isn't an AMI's id"));
    assert_eq!(self::log(&dir).matches("ec2 run-instances").count(), 1);
}

#[test]
fn an_image_goes_with_its_snapshot_and_only_if_its_links() {
    let dir = TempDir::new("images-remove");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    common::fake(&dir, &ec2());
    call(
        &dir,
        "image-remove",
        serde_json::json!({"account": account(), "place": "eu-west-2", "image": "ami-0a1",
            "owner": "github-7"}),
    )
    .unwrap();
    let log = log(&dir);
    assert!(
        log.contains("ec2 deregister-image --image-id ami-0a1"),
        "{log}"
    );
    assert!(
        log.contains("ec2 delete-snapshot --snapshot-id snap-0b2"),
        "{log}"
    );
    // Another owner's, or an AMI Link didn't make, stays.
    for (image, owner) in [("ami-0a1", "github-8"), ("ami-0e1", "github-7")] {
        let refused = call(
            &dir,
            "image-remove",
            serde_json::json!({"account": account(), "image": image, "owner": owner}),
        );
        assert!(
            refused.unwrap_err().contains("isn't one Link made for you"),
            "{image}"
        );
    }
    assert_eq!(self::log(&dir).matches("deregister-image").count(), 1);
    // Gone already is done.
    call(
        &dir,
        "image-remove",
        serde_json::json!({"account": account(), "image": "ami-0d0", "owner": "github-7"}),
    )
    .unwrap();
}
