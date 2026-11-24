//! Finding boxes and reaching them: by their tags, everywhere but a
//! denied region, another computer let in, and SSH over Systems Manager.

use super::*;

/// Regions an organization's policy denies are skipped when looking in
/// every region, named in the reply, not a failure of the whole list;
/// one asked for by name still fails.
#[test]
fn a_denied_region_is_skipped_when_looking_everywhere() {
    let dir = TempDir::new("adapter-denied");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let script = r#"case "$*" in
  "ec2 describe-regions") echo '{"Regions": [{"RegionName": "us-east-1"}, {"RegionName": "us-east-2"}]}' ;;
  "ec2 describe-instances --region us-east-1 "*) echo 'An error occurred (UnauthorizedOperation) when calling the DescribeInstances operation: explicit deny in a service control policy' >&2; exit 254 ;;
  "ec2 describe-instances --region us-east-2 "*) echo '{"Reservations": [{"Instances": [{"InstanceId": "i-2", "InstanceType": "t3.micro", "State": {"Name": "running"}, "Tags": [{"Key": "lnk-box", "Value": "aws-1"}, {"Key": "lnk-machine", "Value": "p8r2c4n6w0zt"}]}]}]}' ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac"#;
    let aws = fake(&dir, script);
    let account =
        serde_json::json!({"name": "aws", "kind": "aws", "place": "us-east-2", "env": {}});
    let found = call(
        &dir,
        &aws,
        "list",
        serde_json::json!({"account": account, "owner": "github-7"}),
    );
    assert_eq!(found["boxes"][0]["id"], "i-2");
    // The skipped region, typed in the reply for whoever asked to say.
    assert_eq!(found["denied"], serde_json::json!(["us-east-1"]));
    let mut child = lnk_aws(&dir, &aws)
        .arg("list")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let request =
        serde_json::json!({"account": account, "owner": "github-7", "places": ["us-east-1"]});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("UnauthorizedOperation"));
}

/// A box found from another computer: by its owner tag in every enabled
/// region, its key pushed with Instance Connect, its host key kept in a
/// tag and read back.
#[test]
fn boxes_are_found_by_their_tags_and_let_another_computer_in() {
    let dir = TempDir::new("adapter-found");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let script = r#"case "$*" in
  "ec2 describe-regions") echo '{"Regions": [{"RegionName": "eu-west-1"}, {"RegionName": "us-east-1"}]}' ;;
  "ec2 describe-instances --region eu-west-1 --filters Name=tag:lnk-owner,Values=github-7 "*) echo '{"Reservations": [{"Instances": [{"InstanceId": "i-9", "InstanceType": "t3.small", "State": {"Name": "stopped"}, "Tags": [{"Key": "lnk-box", "Value": "aws-1"}, {"Key": "lnk-machine", "Value": "p8r2c4n6w0zt"}, {"Key": "lnk-owner", "Value": "github-7"}]}]}]}' ;;
  "ec2 describe-instances --region eu-west-1 --filters Name=tag-key,Values=lnk-owner "*) echo '{"Reservations": [{"Instances": [{"InstanceId": "i-9", "InstanceType": "t3.small", "State": {"Name": "running"}, "Tags": [{"Key": "lnk-machine", "Value": "p8r2c4n6w0zt"}, {"Key": "lnk-owner", "Value": "github-7"}]}, {"InstanceId": "i-8", "InstanceType": "t3.small", "State": {"Name": "running"}, "Tags": [{"Key": "lnk-machine", "Value": "k3v9q2x7m1ab"}, {"Key": "lnk-owner", "Value": "acct-9"}]}]}]}' ;;
  "ec2 describe-instances --region us-east-1 "*) echo '{"Reservations": []}' ;;
  "ec2 describe-instances --instance-ids i-kept") echo '{"Reservations": [{"Instances": [{"Tags": [{"Key": "lnk-host-key", "Value": "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKEPTkeptKEPTkeptKEPTkeptKEPTkeptKEPTkeptKEPT"}]}]}]}' ;;
  "ec2-instance-connect send-ssh-public-key "*) for a in "$@"; do case "$a" in file://*) cp "${a#file://}" "$(dirname "$0")/pushed" ;; esac; done; echo '{"Success": true}' ;;
  "ec2 create-tags "*) ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac"#;
    let aws = fake(&dir, script);
    let account = serde_json::json!({
        "name": "aws", "kind": "aws", "place": "eu-west-1",
        "env": {"AWS_PROFILE": "default"}
    });
    let found = call(
        &dir,
        &aws,
        "list",
        serde_json::json!({"account": account, "owner": "github-7"}),
    );
    assert_eq!(
        found["boxes"],
        serde_json::json!([{
            "id": "i-9", "name": "aws-1", "machine_id": "p8r2c4n6w0zt",
            "owner": "github-7", "place": "eu-west-1", "state": "stopped",
            "size": "t3.small", "stopped_by_cloud": false, "tags":{"lnk-box":"aws-1","lnk-machine":"p8r2c4n6w0zt","lnk-owner":"github-7"}
        }])
    );
    // With no owner, every owner's boxes, each saying whose.
    let every = call(
        &dir,
        &aws,
        "list",
        serde_json::json!({"account": account, "places": ["eu-west-1"]}),
    );
    let owners: Vec<_> = every["boxes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["id"].as_str().unwrap(), b["owner"].as_str().unwrap()))
        .collect();
    assert_eq!(owners, [("i-9", "github-7"), ("i-8", "acct-9")]);
    // Only the regions asked, when some are.
    call(
        &dir,
        &aws,
        "list",
        serde_json::json!({"account": account, "owner": "github-7", "places": ["us-east-1"]}),
    );

    let key = "ssh-ed25519 AAAAnew lnk k3v9q2x7m1ab danas-mbp";
    let pushed = call(
        &dir,
        &aws,
        "add-key",
        serde_json::json!({"account": account, "id": "i-9", "public_key": key}),
    );
    assert_eq!(pushed["temporary"], true);
    assert_eq!(std::fs::read_to_string(dir.join("pushed")).unwrap(), key);

    call(
        &dir,
        &aws,
        "save-hostkeys",
        serde_json::json!({"account": account, "id": "i-9", "host_keys": [
            "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTY",
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGx1pZlbXq0WyU1b4v5r0Ke+ZtrQ2mX5d9J8lJk3aB7c"
        ]}),
    );
    let kept = call(
        &dir,
        &aws,
        "hostkeys",
        serde_json::json!({"account": account, "id": "i-kept"}),
    );
    assert_eq!(
        kept["host_keys"],
        serde_json::json!([
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKEPTkeptKEPTkeptKEPTkeptKEPTkeptKEPTkeptKEPT"
        ])
    );
    call(
        &dir,
        &aws,
        "tag",
        serde_json::json!({"account": account, "id": "i-9", "tags": {"lnk-box": "aws-1"}}),
    );

    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    let calls: Vec<_> = log.lines().filter(|l| !l.starts_with("AWS_")).collect();
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.starts_with("ec2 describe-instances --region"))
            .count(),
        4,
        "{log}"
    );
    assert!(calls.iter().any(|c| c.starts_with(
        "ec2-instance-connect send-ssh-public-key --instance-id i-9 --instance-os-user ubuntu \
         --ssh-public-key file://"
    )));
    // The ed25519 key, the one a tag holds.
    assert!(
        calls.contains(
            &r#"ec2 create-tags --resources i-9 --tags [{"Key":"lnk-host-key","Value":"ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGx1pZlbXq0WyU1b4v5r0Ke+ZtrQ2mX5d9J8lJk3aB7c"}]"#
        ),
        "{log}"
    );
    assert!(calls.contains(
        &r#"ec2 create-tags --resources i-9 --tags [{"Key":"lnk-box","Value":"aws-1"}]"#
    ));
}

/// `lnk-aws tunnel` is a box's SSH over Systems Manager, as ssh's
/// ProxyCommand runs it: `aws ssm start-session` to the box's port 22
/// on this program's own stdin and stdout, acting as the account `lnk
/// cloud env` names, its keys in its environment and never its command
/// line. An account there isn't, or other arguments, are refused.
#[test]
fn a_tunnel_carries_ssh_over_systems_manager() {
    let dir = TempDir::new("adapter-tunnel");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    // Systems Manager, carrying what it's given straight back.
    let aws = fake(&dir, r#"case "$*" in "ssm start-session "*) cat ;; esac"#);
    let lnk = dir.join("lnk");
    link_test_kit::write_program(
        &lnk,
        r#"#!/bin/sh
case "$*" in
  "cloud env work --json") echo '{"name": "work", "kind": "aws", "place": "eu-west-1", "env": {"AWS_ACCESS_KEY_ID": "AKIAEXAMPLEKEY12", "AWS_SECRET_ACCESS_KEY": "the-secret"}}' ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    let tunnel = |args: &[&str]| {
        let mut child = lnk_aws(&dir, &aws)
            .env("LNK_EXE", &lnk)
            .arg("tunnel")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = child
            .stdin
            .take()
            .unwrap()
            .write_all(b"SSH-2.0-OpenSSH_9.6\r\n");
        child.wait_with_output().unwrap()
    };
    let output = tunnel(&[
        "--cloud",
        "work",
        "--zone",
        "us-east-2",
        "--id",
        "i-0abc123",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"SSH-2.0-OpenSSH_9.6\r\n");
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    assert_eq!(
        log.lines().next().unwrap(),
        "ssm start-session --target=i-0abc123 --document-name AWS-StartSSHSession --parameters portNumber=22"
    );
    assert!(log.contains("AWS_REGION=us-east-2"), "{log}");
    assert!(log.contains("AWS_SECRET_ACCESS_KEY=the-secret"), "{log}");
    assert!(!log.contains("someone-else"), "{log}");

    let output = tunnel(&[
        "--cloud",
        "nobody",
        "--zone",
        "us-east-2",
        "--id",
        "i-0abc123",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no cloud account named \"nobody\""));
    let output = tunnel(&[
        "--cloud",
        "work",
        "--zone",
        "us-east-2",
        "--id",
        "i-1;reboot",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("isn't letters, digits"));
    let output = tunnel(&["--id", "i-0abc123"]);
    assert!(!output.status.success());
}
