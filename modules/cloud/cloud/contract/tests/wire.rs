//! The generated wire reads today's JSON and writes it back unchanged,
//! for every message a cloud adapter reads or prints: what an adapter
//! built before the contract was declared says is still what it says.

link_test_kit::program!(unit);

use link_cloud_contract::{Account, Reply, Request};
use serde_json::{Value, json};

/// `value` read as `T` and written back.
fn again<T: serde::de::DeserializeOwned + serde::Serialize>(value: &Value) -> Value {
    let read: T = serde_json::from_value(value.clone()).unwrap();
    serde_json::to_value(&read).unwrap()
}

fn account() -> Value {
    json!({
        "name": "work", "kind": "aws", "place": "eu-west-1",
        "identity": "AWS account 1234", "login": "your aws login (profile work)",
        "region": "eu-west-1", "profile": "work",
        "settings": {"AWS_PROFILE": "work"},
        "env": {"AWS_ACCESS_KEY_ID": "AKIA", "AWS_SECRET_ACCESS_KEY": "s3cret"}
    })
}

#[test]
fn an_account_round_trips() {
    assert_eq!(again::<Account>(&account()), account());
}

#[test]
fn every_request_round_trips() {
    let spec = json!({
        "attach": {"network": "control", "identity": "service", "address": "fixed", "disks": ["data"]},
        "self_remove": true, "deadline": "4h", "name": "web-1", "id": "m-1",
        "tags": {"lnk-box": "web-1"}, "size": "t3.small", "user_data": "#cloud-config\n",
        "public_key": "ssh-ed25519 AAAA", "os": "ubuntu-24.04", "arch": "x86_64",
        "disk_gb": 40, "disk_type": "fast", "spot": true, "gpus": 1,
        "gpu_model": "nvidia-l4", "tunnel": true
    });
    for request in [
        json!({"account": account()}),
        json!({"account": account(), "place": "eu-west-1", "spec": spec}),
        json!({"account": account(), "place": "eu-west-1", "id": "i-1"}),
        json!({"account": account(), "owner": "acct-1", "places": ["eu-west-1", "us-east-1"]}),
        json!({"account": account(), "id": "i-1", "public_key": "ssh-ed25519 AAAA"}),
        json!({"account": account(), "id": "i-1", "host_keys": ["ssh-ed25519 AAAA"]}),
        json!({"account": account(), "id": "i-1", "tags": {"lnk-group": "g"}}),
        json!({"account": account(), "bucket": "b", "labels": {"lnk": "yes"}}),
        json!({"account": account(), "id": "i-1", "image": "web-ready"}),
        json!({"account": account(), "bucket": "b", "object": "out/1", "expires": "1h"}),
        json!({"account": account(), "bucket": "b", "objects": ["a", "b"]}),
    ] {
        assert_eq!(again::<Request>(&request), request);
    }
}

#[test]
fn every_reply_round_trips() {
    for reply in [
        json!({}),
        json!({"place": "eu-west-1", "size": "t3.small"}),
        json!({"id": "i-1", "attachments": true}),
        json!({"ip": "203.0.113.7"}),
        json!({"state": "stopped", "stopped_by_cloud": true}),
        json!({"host_keys": ["ssh-ed25519 AAAA"]}),
        json!({"boxes": [{
            "tags": {"lnk-box": "web-1"}, "deadline": 1900000000, "id": "i-1", "name": "web-1",
            "machine_id": "m-1", "owner": "acct-1", "place": "eu-west-1a", "state": "running",
            "ip": "203.0.113.7", "size": "t3.small", "stopped_by_cloud": false, "spot": true,
            "disk_gb": 40, "disk_type": "fast", "os": "ubuntu-24.04", "arch": "x86_64"
        }], "denied": ["ap-east-1"]}),
        json!({"boxes": [{"id": "i-2", "name": "old", "machine_id": "", "place": "x", "state": "gone", "size": ""}]}),
        json!({"temporary": true}),
        json!({"buckets": [{"bucket": "b", "place": "eu-west-1", "labels": {"lnk": "yes"}}]}),
        json!({"sizes": [
            {"size": "g6.xlarge", "cpus": 4, "memory_mb": 16384, "gpus": 1, "gpu_model": "nvidia-l4", "arch": "x86_64", "price": 0.8},
            {"size": "t3.small", "cpus": 2, "memory_mb": 2048, "gpus": 0, "arch": "x86_64"}
        ], "rates": {"balanced_disk": 0.0001, "fast_disk": 0.0002, "address": 0.005, "egress": 0.09}}),
        json!({"credential": {"type": "s3", "secret_access_key": "s3cret"}, "env": {"RCLONE_X": "y"}, "writes": false, "message": "full"}),
        json!({"images": [{
            "id": "ami-1", "name": "web", "place": "eu-west-1", "state": "ready", "disk_gb": 20,
            "arch": "x86_64", "created": "2026-10-01T10:00:00.000Z", "hash": "3f2a9c"
        }]}),
        json!({"images": [{"id": "lnk-web", "name": "web", "place": "", "state": "pending"}]}),
        json!({"upload": {"url": "https://example.com/signed", "headers": {"x-amz-acl": "private"}}}),
        json!({"contents": "hello"}),
    ] {
        assert_eq!(again::<Reply>(&reply), reply);
    }
}
