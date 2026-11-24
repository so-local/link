//! Spot boxes: made with their request cancelled first, a refusal saying
//! where to raise the quota, and one AWS took back coming back.

use super::*;

/// A spot box with GPUs: NVIDIA's image, a persistent request stopping
/// it, and its request cancelled before it's terminated.
#[test]
fn a_spot_box_is_made_and_its_request_cancelled_first() {
    let dir = TempDir::new("adapter-spot");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let aws = fake(
        &dir,
        &EC2.replace(
            r#"  "ec2 describe-instances "*)"#,
            r#"  "ec2 describe-instances --instance-ids i-s") echo '{"Reservations": [{"Instances": [{"InstanceId": "i-s", "InstanceType": "g6.xlarge", "SpotInstanceRequestId": "sir-1", "InstanceLifecycle": "spot", "Architecture": "x86_64", "ImageId": "ami-9", "RootDeviceName": "/dev/sda1", "BlockDeviceMappings": [{"DeviceName": "/dev/sda1", "Ebs": {"VolumeId": "vol-1"}}], "State": {"Name": "stopped"}, "Tags": [{"Key": "lnk-box", "Value": "gpu-1"}]}]}]}' ;;
  "ec2 describe-volumes --volume-ids vol-1") echo '{"Volumes": [{"Size": 100, "VolumeType": "gp3"}]}' ;;
  "ec2 describe-images --image-ids ami-9") echo '{"Images": [{"Name": "Deep Learning Base OSS Nvidia Driver GPU AMI (Ubuntu 24.04) 20250101"}]}' ;;
  "ec2 describe-instances "*)"#,
        ),
    );
    let account =
        serde_json::json!({"name": "aws", "kind": "aws", "place": "eu-west-1", "env": {}});
    let spec = serde_json::json!({
        "name": "gpu-1", "size": "g6.xlarge", "user_data": "", "public_key": "ssh-ed25519 AAAA",
        "os": "ubuntu-24.04", "disk_gb": 100, "disk_type": "balanced", "spot": true,
        "gpus": 1, "gpu_model": "nvidia-l4"
    });
    call(
        &dir,
        &aws,
        "launch",
        serde_json::json!({"account": account, "spec": spec}),
    );
    // As AWS says it is, for `lnk box spec`.
    let described = call(
        &dir,
        &aws,
        "describe",
        serde_json::json!({"account": account, "id": "i-s"}),
    );
    assert_eq!(
        described["boxes"][0],
        serde_json::json!({
            "id": "i-s", "name": "gpu-1", "machine_id": "", "place": "eu-west-1",
            "state": "stopped", "size": "g6.xlarge", "stopped_by_cloud": false, "spot": true, "disk_gb": 100,
            "disk_type": "balanced", "os": "ubuntu-24.04", "arch": "x86_64", "tags":{"lnk-box":"gpu-1"}
        })
    );
    call(
        &dir,
        &aws,
        "remove",
        serde_json::json!({"account": account, "id": "i-s"}),
    );
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    let calls: Vec<_> = log.lines().filter(|l| !l.starts_with("AWS_")).collect();
    assert!(calls.iter().any(|c| c.contains(
        "ssm get-parameter --name /aws/service/deeplearning/ami/x86_64/base-oss-nvidia-driver-gpu-ubuntu-24.04/"
    )));
    let run = calls
        .iter()
        .find(|c| c.starts_with("ec2 run-instances"))
        .unwrap();
    assert!(run.contains(r#""VolumeSize":100"#), "{run}");
    assert_eq!(
        common::json_argument(run, "--instance-market-options"),
        serde_json::json!({"MarketType": "spot", "SpotOptions": {"InstanceInterruptionBehavior": "stop", "SpotInstanceType": "persistent"}})
    );
    assert!(
        run.contains(r#"{"ResourceType":"spot-instances-request""#),
        "{run}"
    );
    let cancel = calls
        .iter()
        .position(|c| *c == "ec2 cancel-spot-instance-requests --spot-instance-request-ids sir-1")
        .expect("the request cancelled");
    let terminate = calls
        .iter()
        .position(|c| c.starts_with("ec2 terminate-instances --instance-ids i-s"))
        .unwrap();
    assert!(cancel < terminate);
    // An arm64 box with GPUs (no image has both) is refused before
    // anything is made.
    let mut child = lnk_aws(&dir, &aws)
        .arg("launch")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let arm = serde_json::json!({"account": account, "spec": {
        "name": "a", "size": "t4g.small", "user_data": "", "public_key": "k", "arch": "arm64",
        "gpus": 1
    }});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(arm.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("arm64"));
}

/// A GPU or spot box AWS refuses: what AWS said, then where to raise
/// the quota, said by the adapter (the box module names no cloud).
#[test]
fn a_refused_spot_box_says_where_to_raise_the_quota() {
    let dir = TempDir::new("adapter-quota");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let aws = fake(
        &dir,
        &EC2.replace(
            r#"  "ec2 run-instances "*) echo '{"Instances": [{"InstanceId": "i-1"}]}' ;;"#,
            r#"  "ec2 run-instances "*) echo 'An error occurred (MaxSpotInstanceCountExceeded)' >&2; exit 254 ;;"#,
        ),
    );
    let account =
        serde_json::json!({"name": "aws", "kind": "aws", "place": "eu-west-1", "env": {}});
    let spec = serde_json::json!({
        "name": "a", "size": "t3.small", "user_data": "", "public_key": "ssh-ed25519 AAAA",
        "spot": true
    });
    let said = call_fails(
        &dir,
        &aws,
        "launch",
        serde_json::json!({"account": account, "spec": spec}),
    );
    assert!(said.contains("MaxSpotInstanceCountExceeded"), "{said}");
    assert!(said.contains("Service Quotas (EC2)"), "{said}");
}

/// A spot machine AWS took back isn't started (AWS refuses): it comes back
/// on its own, as its persistent request asks, and the adapter says so.
#[test]
fn a_spot_machine_aws_took_back_comes_back_on_its_own() {
    let dir = TempDir::new("adapter-taken");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let aws = fake(
        &dir,
        &EC2.replace(
            r#"  "ec2 describe-instances "*)"#,
            r#"  "ec2 describe-instances --instance-ids i-taken") echo '{"Reservations": [{"Instances": [{"InstanceId": "i-taken", "State": {"Name": "stopped"}, "StateReason": {"Code": "Server.SpotInstanceShutdown"}}]}]}' ;;
  "ec2 start-instances "*) echo '{}' ;;
  "ec2 describe-instances "*)"#,
        ),
    );
    let account =
        serde_json::json!({"name": "aws", "kind": "aws", "place": "eu-west-1", "env": {}});
    let said = call_fails(
        &dir,
        &aws,
        "start",
        serde_json::json!({"account": account, "id": "i-taken"}),
    );
    assert!(said.contains("starts it again itself"), "{said}");
    let state = call(
        &dir,
        &aws,
        "state",
        serde_json::json!({"account": account, "id": "i-taken"}),
    );
    assert_eq!(
        state,
        serde_json::json!({"state": "stopped", "stopped_by_cloud": true})
    );
    // One a person stopped starts as ever.
    let started = call(
        &dir,
        &aws,
        "start",
        serde_json::json!({"account": account, "id": "i-1"}),
    );
    assert_eq!(started["ip"], "203.0.113.5");
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    assert!(
        !log.contains("start-instances --instance-ids i-taken"),
        "{log}"
    );
    assert!(log.contains("start-instances --instance-ids i-1"), "{log}");
}
