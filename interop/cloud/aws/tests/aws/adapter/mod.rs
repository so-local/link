//! `lnk-aws` with a fake `aws`: what reaches the CLI, and what comes back.

use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use link_test_kit::TempDir;

use crate::common;

mod buckets;
mod finding;
mod making;
mod spot;

use common::{fake, lnk_aws};

#[test]
fn a_key_is_checked_and_described_never_on_a_command_line() {
    let dir = TempDir::new("adapter-connect");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let aws = fake(
        &dir,
        r#"echo '{"Account": "123456789012", "Arn": "arn:aws:iam::123456789012:user/me"}'"#,
    );
    // No terminal to ask on: the secret comes from its variable, and
    // stdout is only the account.
    let output = lnk_aws(&dir, &aws)
        .args([
            "connect",
            "--name",
            "work",
            "--key-id",
            "AKIAEXAMPLEKEY12",
            "--region",
            "eu-west-1",
        ])
        .env("AWS_SECRET_ACCESS_KEY", "the-secret")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let account: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(account["name"], "work");
    assert_eq!(account["kind"], "aws");
    assert_eq!(account["place"], "eu-west-1");
    assert_eq!(
        account["identity"],
        "AWS account 123456789012 (arn:aws:iam::123456789012:user/me)"
    );
    assert_eq!(account["login"], "access key AKIA…EY12");
    assert_eq!(account["env"]["AWS_SECRET_ACCESS_KEY"], "the-secret");

    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    assert!(log.contains("sts get-caller-identity"), "{log}");
    assert!(log.contains("AWS_ACCESS_KEY_ID=AKIAEXAMPLEKEY12"), "{log}");
    assert!(log.contains("AWS_SECRET_ACCESS_KEY=the-secret"), "{log}");
    assert!(!log.contains("someone-else"), "{log}");
    assert!(!log.lines().next().unwrap().contains("the-secret"), "{log}");

    // A key AWS refuses is an error, not an account.
    let refusing = fake(&dir, "echo 'InvalidClientTokenId' >&2; exit 254");
    let output = lnk_aws(&dir, &refusing)
        .args(["connect", "--key-id", "AKIABADKEY000000"])
        .env("AWS_SECRET_ACCESS_KEY", "rejected-secret-canary")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("InvalidClientTokenId"), "{error}");
    assert!(!error.contains("rejected-secret-canary"));
}

/// Answers each call the way EC2 would, from a fixed script: an
/// instance is terminated once it's asked to be.
const EC2: &str = r#"case "$*" in
  "ec2 describe-instances --instance-ids "*) if [ -f "$(dirname "$0")/terminated" ]; then echo '{"Reservations": [{"Instances": [{"State": {"Name": "terminated"}}]}]}'; exit 0; fi ;;
esac
case "$*" in
  "ssm get-parameter --name "*) echo '{"Parameter": {"Value": "ami-123"}}' ;;
  "ec2 describe-images --image-ids ami-123") echo '{"Images": [{"RootDeviceName": "/dev/sda1", "BlockDeviceMappings": [{"DeviceName": "/dev/sda1", "Ebs": {"VolumeSize": 8}}]}]}' ;;
  "ec2 describe-key-pairs --filters "*) echo '{"KeyPairs": [{"KeyName": "lnk-box-3f2a9c"}, {"KeyName": "lnk-box-older1"}, {"KeyName": "theirs"}]}' ;;
  "ec2 describe-key-pairs "*) exit 254 ;;
  "ec2 import-key-pair "*) echo '{}' ;;
  "ec2 describe-vpcs "*) echo '{"Vpcs": [{"VpcId": "vpc-1"}]}' ;;
  "ec2 describe-security-groups "*) if [ -f "$(dirname "$0")/group" ]; then echo '{"SecurityGroups": [{"GroupId": "sg-1"}]}'; else echo '{"SecurityGroups": []}'; fi ;;
  "ec2 create-security-group "*) touch "$(dirname "$0")/group"; echo '{"GroupId": "sg-1"}' ;;
  "ec2 authorize-security-group-ingress "*) echo '{}' ;;
  "ec2 run-instances "*) echo '{"Instances": [{"InstanceId": "i-1"}]}' ;;
  "ec2 describe-instances --instance-ids i-gone") echo 'An error occurred (InvalidInstanceID.NotFound)' >&2; exit 254 ;;
  "ec2 describe-instances --filters "*) echo '{"Reservations": []}' ;;
  "ec2 describe-instances "*) echo '{"Reservations": [{"Instances": [{"PublicIpAddress": "203.0.113.5", "State": {"Name": "running"}}]}]}' ;;
  "ec2 get-console-output --instance-id i-1 --latest") echo 'not supported for this instance type' >&2; exit 1 ;;
  "ec2 get-console-output --instance-id i-1") printf '%s\n' '{"Output": "-----BEGIN SSH HOST KEY KEYS-----\necdsa-sha2-nistp256 AAAAC3NzaC1lZDI1NTE5AAAAIGx1pZlbXq0WyU1b4v5r0Ke+ZtrQ2mX5d9J8lJk3aB7c root@ip\n-----END SSH HOST KEY KEYS-----\n"}' ;;
  "ec2 describe-spot-instance-requests "*) echo '{"SpotInstanceRequests": [{"SpotInstanceRequestId": "sir-gone", "InstanceId": "i-gone"}, {"SpotInstanceRequestId": "sir-open"}]}' ;;
  "ec2 cancel-spot-instance-requests "*) echo '{}' ;;
  "ec2 terminate-instances "*) touch "$(dirname "$0")/terminated"; echo '{}' ;;
  "ec2 delete-security-group "*|"ec2 delete-key-pair "*) echo '{}' ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac"#;

fn call(dir: &Path, aws: &Path, command: &str, request: serde_json::Value) -> serde_json::Value {
    let mut child = lnk_aws(dir, aws)
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
    assert!(
        output.status.success(),
        "{command}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Runs `command` with `request`, which must fail; what it said on stderr.
fn call_fails(dir: &Path, aws: &Path, command: &str, request: serde_json::Value) -> String {
    let mut child = lnk_aws(dir, aws)
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
    assert!(!output.status.success(), "{command} succeeded");
    String::from_utf8_lossy(&output.stderr).into_owned()
}
