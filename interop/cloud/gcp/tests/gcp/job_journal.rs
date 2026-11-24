//! Actual provider launch protocol with an isolated native IAM counterpart.
use link_test_kit::{TempDir, env, process::ChildGroup};
use serde_json::{Value, json};
use std::io::Read;
use std::path::Path;
use std::process::{Output, Stdio};

const NATIVE: &str = r#"#!/usr/bin/env python3
import json,os,sys,fcntl
from pathlib import Path
root=Path(__file__).parent
lock=(root/'state.lock').open('a+');fcntl.flock(lock,fcntl.LOCK_EX)
path=root/'state.json'; state=json.loads(path.read_text());args=sys.argv[1:];state['calls'].append(args)
def save():path.write_text(json.dumps(state))
def reply(v):save();print(json.dumps(v));sys.exit(0)
def refuse(operation):save();print('ERROR: (gcloud.'+operation+') NOT_FOUND: Unknown identity.',file=sys.stderr);sys.exit(1)
def events():
 p=root/'job-identities.jsonl'
 return [json.loads(line) for line in p.read_text().splitlines()] if p.exists() else []
if args[:3]==['iam','roles','describe']:
 if state['mode']=='missing-role':refuse('iam.roles.describe')
 if '--project=exact-project' not in args:save();print('ERROR: (gcloud.iam.roles.describe) Missing required argument [--organization or --project]: Should specify the project or organization name for custom roles.',file=sys.stderr);sys.exit(1)
 role={'name':'projects/exact-project/roles/lnkBoxEnd','etag':'preserved-role','description':'shared prerequisite','includedPermissions':['compute.instances.delete']}
 if state['mode']=='wide-role':role['includedPermissions'].append('compute.instances.start')
 reply(role)
elif args[:2]==['projects','describe']:reply({'projectId':'exact-project','projectNumber':'100200300400'})
elif args[:3]==['iam','service-accounts','describe']:
 sa=state['sa']
 if state['mode']=='unreadable':save();print('ERROR: (gcloud.iam.service-accounts.describe) PERMISSION_DENIED: not found',file=sys.stderr);sys.exit(1)
 if sa:reply(sa)
 refuse('iam.service-accounts.describe')
elif args[:2]==['projects','get-iam-policy']:reply({'bindings':[{'role':'roles/viewer','members':['user:foreign@example.com']}]})
elif args[:3]==['iam','service-accounts','create']:
 before=events();assert len(before)==1 and before[0]['event']=='intent','no durable intent before allocation'
 intent=before[0]['value'];assert intent['project_number']=='100200300400'
 description=next(a.split('=',1)[1] for a in args if a.startswith('--description='));assert description.endswith(intent['nonce'])
 state['sa']={'email':intent['email'],'uniqueId':'100100100100100100001','description':description};save()
 if state['mode']=='lost-create':print('response lost',file=sys.stderr);sys.exit(1)
 reply({})
elif args[:2]==['projects','add-iam-policy-binding']:
 before=events();assert [e['event'] for e in before]==['intent','account'];assert before[1]['value']['account']['unique_id']==state['sa']['uniqueId']
 state['granted']=True;reply({})
elif args[:3]==['compute','firewall-rules','describe']:
 if state['mode']=='missing-rule' and args[3]=='lnk-box-deny':refuse('compute.firewall-rules.describe')
 reply({})
elif args[:3]==['compute','firewall-rules','delete']:state['rules'].remove(args[3]);reply({})
elif args[:3]==['compute','instances','delete']:
 if state['mode']=='cleanup-refused':save();print('native VM removal refused',file=sys.stderr);sys.exit(1)
 state['vm']=False;reply({})
elif args[:3]==['compute','instances','list']:reply([])
elif args[:3]==['compute','images','describe-from-family']:reply({'diskSizeGb':'10'})
elif args[:3]==['compute','instances','create']:
 assert [e['event'] for e in events()]==['intent','account','binding']
 if state['mode']=='failed-vm':save();print('VM creation refused',file=sys.stderr);sys.exit(1)
 state['vm']=True;reply([])
else:save();print('unexpected native call '+str(args),file=sys.stderr);sys.exit(2)
"#;

fn run(root: &Path, mode: &str) -> (Output, Value, Vec<Value>) {
    let state = json!({"mode":mode,"calls":[],"sa":null,"granted":false,"vm":false,"rules":["lnk-box-ssh","lnk-box-deny"]});
    std::fs::write(root.join("state.json"), state.to_string()).unwrap();
    link_test_kit::write_program(&root.join("gcloud"), NATIVE).unwrap();
    let request = json!({"account":{"name":"approved-gcp","kind":"gcp","place":"us-central1-a","project":"exact-project","env":{}},
        "spec":{"name":"qa-job","id":"k3v9q2x7m1ab","size":"e2-small","public_key":"ssh-ed25519 fixture","user_data":"#cloud-config\nruncmd: []\n","deadline":"4h","self_remove":true}});
    let output = call(root, "launch", &request);
    let state = serde_json::from_slice(&std::fs::read(root.join("state.json")).unwrap()).unwrap();
    let events = std::fs::read_to_string(root.join("job-identities.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    (output, state, events)
}

fn call(root: &Path, verb: &str, request: &Value) -> Output {
    std::fs::write(root.join("request.json"), request.to_string()).unwrap();
    let mut command = env::command(env!("CARGO_BIN_EXE_lnk-gcp"), root);
    command
        .arg(verb)
        .env("LNK_GCLOUD", root.join("gcloud"))
        .env("LNK_RESOURCE_JOURNAL", root.join("launches.jsonl"))
        .env("LNK_SERVICES", "off")
        .stdin(Stdio::from(
            std::fs::File::open(root.join("request.json")).unwrap(),
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = ChildGroup::spawn(&mut command).unwrap();
    let status = link_test_kit::wait::eventually(
        "journaled native GCP launch",
        std::time::Duration::from_secs(15),
        || child.try_status().unwrap(),
    );
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .take_stdout()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    child
        .take_stderr()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    Output {
        status,
        stdout,
        stderr,
    }
}

#[test]
fn journaled_launch_records_immutable_native_iam_before_the_vm() {
    let dir = TempDir::new("gcp-journal-launch");
    let (output, state, events) = run(&dir, "success");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(state["vm"], true);
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["value"]["account"], "approved-gcp");
    assert_eq!(
        events[1]["value"]["account"]["unique_id"],
        "100100100100100100001"
    );
    assert!(
        !state["calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a[0] == "iam" && a[1] == "roles" && a[2] == "create")
    );
}

#[test]
fn lost_create_and_failed_vm_retain_the_exact_recovery_journal() {
    for (mode, count) in [("lost-create", 1), ("failed-vm", 3)] {
        let dir = TempDir::new("gcp-journal-failure");
        let (output, state, events) = run(&dir, mode);
        assert!(!output.status.success(), "{mode}");
        assert_eq!(events.len(), count, "{mode}");
        assert!(!state["sa"].is_null());
        assert_eq!(state["vm"], false);
    }
}

#[test]
fn missing_or_wide_role_and_unreadable_identity_refuse_without_allocation() {
    for mode in ["missing-role", "wide-role", "unreadable", "missing-rule"] {
        let dir = TempDir::new("gcp-journal-refusal");
        let (output, state, events) = run(&dir, mode);
        assert!(!output.status.success(), "{mode}");
        assert!(events.is_empty(), "{mode}");
        assert!(state["sa"].is_null());
        assert!(
            !state["calls"].as_array().unwrap().iter().any(|a| a
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s == "create")),
            "{mode}"
        );
    }
}

#[test]
fn only_the_provider_explicitly_advertises_job_identity_journaling() {
    let dir = TempDir::new("gcp-journal-capability");
    let output = env::command(env!("CARGO_BIN_EXE_lnk-gcp"), &dir)
        .arg(link_cloud_contract::ownership::SCHEMA_ARGUMENT)
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: link_cloud_contract::ownership::Schema =
        serde_json::from_slice(&output.stdout).unwrap();
    assert!(value.job_identities);
    let generic: link_cloud_contract::ownership::Schema =
        serde_json::from_slice(&link_cloud_contract::ownership::schema_bytes()).unwrap();
    assert!(!generic.job_identities);
}

#[test]
fn journaled_success_and_failure_cleanup_preserve_the_shared_firewall_baseline() {
    for mode in ["success", "failed-vm", "cleanup-refused"] {
        let dir = TempDir::new("gcp-preserved-firewall");
        let (_, _, _) = run(&dir, mode);
        let request = json!({"account":{"name":"approved-gcp","kind":"gcp","place":"us-central1-a","project":"exact-project","env":{}},"id":"lnk-k3v9q2x7m1ab"});
        let removed = call(&dir, "remove", &request);
        assert_eq!(removed.status.success(), mode != "cleanup-refused");
        let cleaned = call(&dir, "cleanup", &request);
        assert!(
            cleaned.status.success(),
            "{}",
            String::from_utf8_lossy(&cleaned.stderr)
        );
        let state: Value =
            serde_json::from_slice(&std::fs::read(dir.join("state.json")).unwrap()).unwrap();
        assert_eq!(state["rules"], json!(["lnk-box-ssh", "lnk-box-deny"]));
        assert!(
            !state["calls"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a[0] == "compute" && a[1] == "firewall-rules" && a[2] == "delete")
        );
        assert!(
            !state["calls"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a[0] == "iam" && a[1] == "service-accounts" && a[2] == "list")
        );
    }
}
