//! Making a box: an instance in the default VPC, its disk at least its
//! image's, what boxes share kept while one is left, and what it's
//! attached to kept when it's replaced.

use super::*;

#[test]
fn a_box_is_an_instance_in_the_default_vpc() {
    let dir = TempDir::new("adapter-box");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let aws = fake(&dir, EC2);
    let account = serde_json::json!({
        "name": "aws", "kind": "aws", "place": "eu-west-1",
        "env": {"AWS_PROFILE": "default", "AWS_REGION": "eu-west-1"}
    });
    let spec = serde_json::json!({
        "name": "work", "size": "t3.small", "user_data": "#cloud-config\n",
        "public_key": "ssh-ed25519 AAAAC3Nza lnk-box"
    });
    let defaults = call(
        &dir,
        &aws,
        "defaults",
        serde_json::json!({"account": account}),
    );
    assert_eq!(
        defaults,
        serde_json::json!({"place": "eu-west-1", "size": "t3.small"})
    );
    let launched = call(
        &dir,
        &aws,
        "launch",
        serde_json::json!({"account": account, "spec": spec}),
    );
    assert_eq!(launched["id"], "i-1");
    // From an lnk box with IDs: named after the ID, tagged.
    call(
        &dir,
        &aws,
        "launch",
        serde_json::json!({"account": account, "spec": {
            "name": "aws-1", "id": "p8r2c4n6w0zt", "size": "t3.small", "user_data": "",
            "tags": {"lnk-box": "aws-1", "lnk-machine": "p8r2c4n6w0zt", "lnk-owner": "github-7"}
        }}),
    );
    let running = call(
        &dir,
        &aws,
        "wait",
        serde_json::json!({"account": account, "id": "i-1"}),
    );
    assert_eq!(running["ip"], "203.0.113.5");
    // The console's latest output where the type keeps it, else AWS's copy.
    let keys = call(
        &dir,
        &aws,
        "hostkeys",
        serde_json::json!({"account": account, "id": "i-1"}),
    );
    assert_eq!(
        keys["host_keys"],
        serde_json::json!([
            "ecdsa-sha2-nistp256 AAAAC3NzaC1lZDI1NTE5AAAAIGx1pZlbXq0WyU1b4v5r0Ke+ZtrQ2mX5d9J8lJk3aB7c"
        ])
    );
    let state = call(
        &dir,
        &aws,
        "state",
        serde_json::json!({"account": account, "id": "i-1"}),
    );
    assert_eq!(state["state"], "running");
    call(
        &dir,
        &aws,
        "remove",
        serde_json::json!({"account": account, "id": "i-1"}),
    );
    call(
        &dir,
        &aws,
        "cleanup",
        serde_json::json!({"account": account, "spec": spec}),
    );

    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    let calls: Vec<_> = log.lines().filter(|l| !l.starts_with("AWS_")).collect();
    assert!(calls.iter().any(|c| c.contains(
        r#""Tags":[{"Key":"Name","Value":"lnk-p8r2c4n6w0zt"},{"Key":"lnk-box","Value":"aws-1"},{"Key":"lnk-machine","Value":"p8r2c4n6w0zt"},{"Key":"lnk-owner","Value":"github-7"}]"#
    )));
    let run = calls
        .iter()
        .find(|c| c.starts_with("ec2 run-instances"))
        .unwrap();
    for part in [
        "--image-id ami-123",
        "--instance-type t3.small",
        "--security-group-ids sg-1",
        "--metadata-options HttpTokens=required",
        r#"{"Key":"lnk-box","Value":"work"}"#,
        "--user-data file://",
        // Its disk carries the same tags: the bill says whose it is.
        r#"--tag-specifications [{"ResourceType":"instance","Tags":["#,
        r#"{"ResourceType":"volume","Tags":["#,
        // Today's disk, from an lnk box before specs.
        r#""VolumeSize":20,"VolumeType":"gp3""#,
    ] {
        assert!(run.contains(part), "{part} in {run}");
    }
    assert!(calls.iter().any(|c| c.starts_with(
        "ec2 authorize-security-group-ingress --group-id sg-1 --protocol tcp --port 22"
    )));
    // An older lnk box's key gets a key pair; today's gives none, and
    // its box trusts Link's certificates instead.
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("ec2 import-key-pair --key-name lnk-box-"))
    );
    assert!(run.contains("--key-name lnk-box-"), "{run}");
    let today = calls
        .iter()
        .filter(|c| c.starts_with("ec2 run-instances"))
        .find(|c| c.contains("lnk-p8r2c4n6w0zt"))
        .unwrap();
    assert!(!today.contains("--key-name"), "{today}");
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.starts_with("ec2 import-key-pair"))
            .count(),
        1,
        "{log}"
    );
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("ec2 delete-security-group --group-id sg-1"))
    );
    // Every computer's key pair goes with the last box; none of anyone's.
    for pair in ["lnk-box-3f2a9c", "lnk-box-older1"] {
        assert!(
            calls.contains(&format!("ec2 delete-key-pair --key-name {pair}").as_str()),
            "{log}"
        );
    }
    assert!(!log.contains("delete-key-pair --key-name theirs"), "{log}");
    assert!(!run.contains("--instance-market-options"), "{run}");
    // Link's spot requests whose box is gone, or which have none, cancelled.
    assert!(
        calls.contains(
            &"ec2 cancel-spot-instance-requests --spot-instance-request-ids sir-gone sir-open"
        ),
        "{log}"
    );
    assert!(log.contains("AWS_REGION=eu-west-1"));
    assert!(!log.contains("someone-else"));
    // Waits ask for the instance's state themselves, never the CLI's
    // waiters, which ask every 15 s.
    assert!(!log.contains("ec2 wait"), "{log}");
    // The user data file is gone once the call returns.
    let file = run
        .split("file://")
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap();
    assert!(!Path::new(file).exists());
}

#[test]
fn what_boxes_share_stays_while_one_is_left() {
    let dir = TempDir::new("adapter-left");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let aws = fake(
        &dir,
        &EC2.replace(
            r#""ec2 describe-instances --filters "*) echo '{"Reservations": []}' ;;"#,
            r#""ec2 describe-instances --filters "*) echo '{"Reservations": [{"Instances": [{"InstanceId": "i-2"}]}]}' ;;"#,
        ),
    );
    let account =
        serde_json::json!({"name": "aws", "kind": "aws", "place": "eu-west-1", "env": {}});
    let spec = serde_json::json!({"name": "a", "size": "t3.small", "user_data": "", "public_key": "ssh-ed25519 AAAA"});
    call(
        &dir,
        &aws,
        "cleanup",
        serde_json::json!({"account": account, "spec": spec}),
    );
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    assert!(log.contains("tag-key,Values=lnk-box"), "{log}");
    assert!(
        !log.contains("delete-security-group") && !log.contains("delete-key-pair"),
        "{log}"
    );
}

/// A box reached only through the tunnel gets a group of its own that
/// lets nothing in, made by an identity refused any ingress rule, and
/// deleted with the last box like the other.
#[test]
fn a_tunnel_box_lets_nothing_in() {
    let dir = TempDir::new("adapter-tunnel-group");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let (allowed, refused) = ("ingress \"*) echo '{}' ;;", "ingress \"*) exit 254 ;;");
    let aws = fake(&dir, &EC2.replace(allowed, refused));
    let request = serde_json::json!({
        "account": {"name": "aws", "kind": "aws", "place": "eu-west-1", "env": {}},
        "spec": {"name": "t", "size": "t3.small", "user_data": "", "public_key": "ssh-ed25519 AAAA", "tunnel": true}
    });
    assert_eq!(call(&dir, &aws, "launch", request.clone())["id"], "i-1");
    call(&dir, &aws, "cleanup", request);
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    for part in [
        "create-security-group --group-name lnk-box-tunnel",
        "Name=group-name,Values=lnk-box Name=vpc-id",
        "delete-security-group --group-id sg-1",
    ] {
        assert!(log.contains(part), "{part}: {log}");
    }
    assert!(!log.contains("authorize-security-group-ingress"), "{log}");
}

/// A GPU box at the default 20 GB: AWS's Deep Learning Base AMI needs a
/// larger disk, so the box gets the image's, said; the old `--size
/// g6.xlarge` with no disk asked still makes a box.
#[test]
fn a_box_gets_at_least_its_images_disk() {
    let dir = TempDir::new("adapter-image-disk");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let aws = fake(
        &dir,
        &EC2.replace(r#""VolumeSize": 8}"#, r#""VolumeSize": 75}"#),
    );
    let account =
        serde_json::json!({"name": "aws", "kind": "aws", "place": "eu-west-1", "env": {}});
    let spec = serde_json::json!({
        "name": "gpu-1", "size": "g6.xlarge", "user_data": "", "public_key": "ssh-ed25519 AAAA",
        "gpus": 1, "gpu_model": "nvidia-l4"
    });
    let output = lnk_aws(&dir, &aws)
        .arg("launch")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(
                serde_json::json!({"account": account, "spec": spec})
                    .to_string()
                    .as_bytes(),
            )?;
            child.wait_with_output()
        })
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains("Its image needs a disk of 75 GB at least: the box gets 75 GB, not 20."),
        "{stderr}"
    );
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    let run = log
        .lines()
        .find(|c| c.starts_with("ec2 run-instances"))
        .unwrap();
    assert_eq!(
        common::json_argument(run, "--block-device-mappings"),
        serde_json::json!([{"DeviceName": "/dev/sda1", "Ebs": {"Encrypted": true, "DeleteOnTermination": true, "VolumeSize": 75, "VolumeType": "gp3"}}])
    );
    // A disk asked larger than the image's is the one asked.
    let mut bigger = spec.clone();
    bigger["disk_gb"] = 200.into();
    call(
        &dir,
        &aws,
        "launch",
        serde_json::json!({"account": account, "spec": bigger}),
    );
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    let run = log
        .lines()
        .rfind(|c| c.starts_with("ec2 run-instances"))
        .unwrap();
    assert!(run.contains(r#""VolumeSize":200"#), "{run}");
}

/// A spot request AWS won't list or cancel is said, and the security
/// group and key pair still go.
#[test]
fn cleanup_goes_on_past_spot_requests() {
    let dir = TempDir::new("adapter-cleanup");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let aws = fake(
        &dir,
        &EC2.replace(
            r#"  "ec2 describe-spot-instance-requests "*) echo"#,
            r#"  "ec2 describe-spot-instance-requests "*) echo 'An error occurred (UnauthorizedOperation)' >&2; exit 254; echo"#,
        ),
    );
    std::fs::write(dir.join("group"), "").unwrap();
    let account =
        serde_json::json!({"name": "aws", "kind": "aws", "place": "eu-west-1", "env": {}});
    let spec = serde_json::json!({"name": "a", "size": "t3.small", "user_data": "", "public_key": "ssh-ed25519 AAAA"});
    let output = lnk_aws(&dir, &aws)
        .arg("cleanup")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(
                serde_json::json!({"account": account, "spec": spec})
                    .to_string()
                    .as_bytes(),
            )?;
            child.wait_with_output()
        })
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("UnauthorizedOperation"), "{stderr}");
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    assert!(
        log.contains("ec2 delete-security-group --group-id sg-1"),
        "{log}"
    );
    assert!(
        log.contains("ec2 delete-key-pair --key-name lnk-box-"),
        "{log}"
    );
}

#[test]
fn an_attached_disk_and_address_survive_replacement() {
    let dir = TempDir::new("attached");
    std::fs::write(dir.join("group"), "").unwrap();
    let extras = r#"case "$*" in
"ec2 describe-subnets "*) echo '{"Subnets":[{"SubnetId":"subnet-1","VpcId":"vpc-1","AvailabilityZone":"eu-west-1a"}]}';exit 0;;
"iam get-instance-profile "*) echo '{"InstanceProfile":{"InstanceProfileName":"service"}}';exit 0;;
"ec2 describe-addresses "*) echo '{"Addresses":[{"AllocationId":"eipalloc-1"}]}';exit 0;;
"ec2 describe-volumes "*) echo '{"Volumes":[{"VolumeId":"vol-1","AvailabilityZone":"eu-west-1a"}]}';exit 0;;
"ec2 wait instance-running "*|"ec2 attach-volume "*|"ec2 modify-instance-attribute "*|"ec2 associate-address "*) echo '{}';exit 0;;
esac
"#;
    let aws = fake(&dir, &format!("{extras}\n{EC2}"));
    let account = serde_json::json!({"name":"aws","kind":"aws","place":"eu-west-1","env":{}});
    let spec = serde_json::json!({"name":"service","size":"t3.small","user_data":"","public_key":"ssh-ed25519 AAAA", "attach":{"network":"subnet-1","identity":"service","address":"eipalloc-1","disks":["vol-1"]}});
    for _ in 0..2 {
        let _ = std::fs::remove_file(dir.join("terminated"));
        call(
            &dir,
            &aws,
            "defaults",
            serde_json::json!({"account":account,"spec":spec}),
        );
        let created = call(
            &dir,
            &aws,
            "launch",
            serde_json::json!({"account":account,"spec":spec}),
        );
        for _ in 0..2 {
            call(
                &dir,
                &aws,
                "remove",
                serde_json::json!({"account":account,"id":created["id"]}),
            );
        }
    }
    let log = std::fs::read_to_string(dir.join("aws.log")).unwrap();
    assert!(
        log.contains("--subnet-id subnet-1")
            && log.contains("--iam-instance-profile Name=service")
            && log.contains("attach-volume --volume-id vol-1"),
        "{log}"
    );
    assert!(
        log.contains("\"DeleteOnTermination\":false")
            && log.contains("associate-address --allocation-id eipalloc-1"),
        "{log}"
    );
    assert!(
        !log.contains("delete-volume")
            && !log.contains("release-address")
            && !log.contains("create-security-group"),
        "{log}"
    );
}
