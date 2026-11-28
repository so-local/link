//! `lnk-gcp` with a fake `gcloud`: what reaches the CLI, and what comes
//! back.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use link_test_kit::TempDir;

const GCLOUD: &str = r#"case "$*" in
  "config get-value account") echo "${FAKE_ACCOUNT:-me@example.com}" ;;
  "auth application-default login"*) echo '{"type": "authorized_user", "refresh_token": "r"}' > "$(dirname "$0")/application_default_credentials.json" ;;
  "info --format=value(config.paths.global_config_dir)") dirname "$0" ;;
  "config get-value project") echo my-project ;;
  "config get-value compute/zone") echo "(unset)" ;;
  "projects describe my-project --format json") echo '{"projectId": "my-project"}' ;;
  "compute firewall-rules describe "*) if [ -f "$(dirname "$0")/rule-$4" ]; then echo '{}'; else echo 'was not found' >&2; exit 1; fi ;;
  "compute firewall-rules create "*) touch "$(dirname "$0")/rule-$4"; echo '[]' ;;
  "compute firewall-rules delete "*) ;;
  "compute images describe-from-family ubuntu-accelerator-"*) echo '{"diskSizeGb": "50"}' ;;
  "compute images describe-from-family "*) echo '{"diskSizeGb": "10"}' ;;
  "compute machine-types list --zones us-central1-a --format json") echo '[{"name": "e2-small", "guestCpus": 2, "memoryMb": 2048}, {"name": "n1-standard-4", "guestCpus": 4, "memoryMb": 15360}]' ;;
  "compute accelerator-types list --filter zone:us-central1-a --format json") echo '[{"name": "nvidia-tesla-t4", "zone": "https://x/zones/us-central1-a", "maximumCardsPerInstance": 4}]' ;;
  "compute networks describe control --format=json") echo '{"name":"control"}' ;;
  "iam service-accounts describe service@my-project.iam.gserviceaccount.com --format=json") echo '{"email":"service@my-project.iam.gserviceaccount.com"}' ;;
  "compute addresses describe fixed --region us-central1 --format=json") echo '{"name":"fixed"}' ;;
  "compute disks describe data --zone us-central1-a --format=json") echo '{"name":"data"}' ;;
  "compute instances create lnk-refused "*) echo "ERROR: Quota 'GPUS_ALL_REGIONS' exceeded." >&2; exit 1 ;;
  "compute instances create "*)
    for a; do case "$a" in user-data=/*) cp "${a#*=}" "$(dirname "$0")/first-boot" ;; esac; done
    echo '[]' ;;
  "iam roles describe lnkBoxEnd "*)
    if [ -f "$(dirname "$0")/role-made" ]; then echo '{"includedPermissions": ["compute.instances.delete"]}'
    else echo 'NOT_FOUND: The role named lnkBoxEnd was not found.' >&2; exit 1; fi ;;
  "iam roles create lnkBoxEnd "*) touch "$(dirname "$0")/role-made" ;;
  "iam service-accounts list --filter=displayName:lnk-job- --format=json") echo '[]' ;;
  "iam service-accounts create lnk-job-"*) ;;
  "projects add-iam-policy-binding "*)
    # IAM can be a moment behind a new account: the first grant is refused.
    if [ ! -f "$(dirname "$0")/granted" ]; then
      touch "$(dirname "$0")/granted"
      echo "ERROR: (gcloud.projects.add-iam-policy-binding) INVALID_ARGUMENT: Service account ${4#--member=serviceAccount:} does not exist." >&2; exit 1
    fi ;;
  "storage sign-url "*)
    if [ -n "$CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE" ]; then
      signing_file=''; for arg; do case "$arg" in --private-key-file=*) signing_file=${arg#*=} ;; esac; done
      [ "$signing_file" = "$CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE" ] || { echo "AttributeError: 'Credentials' object has no attribute 'private_key_id'" >&2; exit 1; }
    fi
    echo '[{"signed_url":"https://storage.googleapis.com/results/job/output.tar?signature=fake"}]' ;;
  "compute zones list --format json") echo '[{"name": "europe-west2-c", "region": "https://x/regions/europe-west2", "status": "UP"}, {"name": "europe-west2-a", "region": "https://x/regions/europe-west2", "status": "DOWN"}, {"name": "europe-west2-b", "region": "https://x/regions/europe-west2", "status": "UP"}, {"name": "europe-west1-a", "region": "https://x/regions/europe-west1", "status": "UP"}]' ;;
  "compute instances describe lnk-gpu-1 "*) echo '{"name": "lnk-gpu-1", "status": "TERMINATED", "labels": {"lnk-box": "gpu-1"}, "zone": "https://x/zones/europe-west2-b", "machineType": "https://x/machineTypes/g2-standard-4", "scheduling": {"provisioningModel": "SPOT"}, "disks": [{"boot": true, "diskSizeGb": "100", "source": "https://x/disks/lnk-gpu-1", "licenses": ["https://x/projects/ubuntu-os-cloud/global/licenses/ubuntu-2204-lts"]}]}' ;;
  "compute disks describe lnk-gpu-1 "*) echo '{"type": "https://x/diskTypes/pd-ssd"}' ;;
  "compute instances describe lnk-kept "*) echo '{"status": "RUNNING", "metadata": {"items": [{"key": "lnk-host-keys", "value": "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKEPTkeptKEPTkeptKEPTkeptKEPTkeptKEPTkeptKEPT"}]}}' ;;
  "compute instances describe "*) printf '%s\n' '{"status": "RUNNING", "metadata": {"items": [{"key": "ssh-keys", "value": "ubuntu:ssh-ed25519 AAAAold lnk abc123 other-mac\nubuntu:ssh-ed25519 AAAAmine dana@laptop"}]}, "networkInterfaces": [{"network": "https://www.googleapis.com/compute/v1/projects/p/global/networks/default", "accessConfigs": [{"natIP": "198.51.100.7"}]}]}' ;;
  "compute instances add-metadata "*) for a in "$@"; do case "$a" in *=/*) cp "${a#*=}" "$(dirname "$0")/added-${a%%=*}" ;; esac; done ;;
  "compute instances add-labels "*) ;;
  "compute instances list --filter labels.lnk-owner:* --format json") echo '[{"name": "lnk-k3v9q2x7m1ab", "labels": {"lnk-box": "gcp-1", "lnk-machine": "k3v9q2x7m1ab", "lnk-owner": "github-7"}, "zone": "zones/europe-west1-b", "status": "RUNNING"}, {"name": "lnk-p8r2c4n6w0zt", "labels": {"lnk-box": "gcp-1", "lnk-machine": "p8r2c4n6w0zt", "lnk-owner": "acct-9"}, "zone": "zones/us-central1-a", "status": "RUNNING"}]' ;;
  "compute instances list --filter labels.lnk-owner=github-7 --format json") echo '[{"name": "lnk-k3v9q2x7m1ab", "labels": {"lnk-box": "gcp-1", "lnk-machine": "k3v9q2x7m1ab", "lnk-owner": "github-7"}, "zone": "https://www.googleapis.com/compute/v1/projects/my-project/zones/europe-west1-b", "status": "TERMINATED", "machineType": "https://www.googleapis.com/compute/v1/projects/my-project/zones/europe-west1-b/machineTypes/e2-small", "networkInterfaces": [{"accessConfigs": [{}]}]}]' ;;
  "compute instances delete "*) ;;
  "compute start-iap-tunnel "*) cat ;;
  "compute instances list "*) echo '[]' ;;
  "compute instances get-serial-port-output lnk-work --zone us-central1-a"|"compute instances get-serial-port-output lnk-kept --zone us-central1-a") printf '%s\n' 'booting' '-----BEGIN SSH HOST KEY KEYS-----' '[  20.1] cloud-init[7]: ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGx1pZlbXq0WyU1b4v5r0Ke+ZtrQ2mX5d9J8lJk3aB7c root@lnk-work' '-----END SSH HOST KEY KEYS-----' ;;
  "storage buckets update gs://lnk-bucket-a1b2c3d4e5f6 --update-labels=lnk-folder-name=moltbot-2") ;;
  "storage buckets list --format=json") echo '[{"name": "lnk-bucket-a1b2c3d4e5f6", "location": "EUROPE-WEST1", "labels": {"lnk-folder": "p8r2c4n6w0zt"}}, {"name": "lnk-bucket-other000000", "location": "US"}]' ;;
  "iam service-accounts create lnk-a1b2c3d4e5f6 "*) echo 'already exists' >&2; exit 1 ;;
  "storage buckets add-iam-policy-binding gs://lnk-bucket-a1b2c3d4e5f6 --member=serviceAccount:lnk-a1b2c3d4e5f6@my-project.iam.gserviceaccount.com --role=roles/storage.objectAdmin") ;;
  "iam service-accounts keys list "*) echo '[{"name": "projects/my-project/serviceAccounts/x/keys/oldkey"}]' ;;
  "iam service-accounts keys delete oldkey "*) ;;
  "iam service-accounts keys create "*) printf '{\n  "type": "service_account",\n  "private_key": "-----BEGIN-----\\nsecret\\n-----END-----\\n"\n}\n' > "$5" ;;
  "storage buckets remove-iam-policy-binding "*) ;;
  "iam service-accounts delete "*) echo 'NOT_FOUND' >&2; exit 1 ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac"#;

fn fake(dir: &Path) -> PathBuf {
    let path = dir.join("gcloud");
    link_test_kit::write_program(
        &path,
        format!(
            "#!/bin/sh\necho \"$*\" >> {log}\nenv | grep -E '^CLOUDSDK_' | sort >> {log}\n\
             case \"$1\" in projects|compute|storage)\n\
               sdk=${{CLOUDSDK_CONFIG:-$HOME/.config/gcloud}}\n\
               if [ -f \"$sdk/configurations/config_default\" ] && grep -Eq 'access_token_file|impersonate_service_account' \"$sdk/configurations/config_default\"; then\n\
                 echo 'SDK configuration selected another principal' >&2; exit 33\n\
               fi ;;\n\
             esac\n{GCLOUD}\n",
            log = dir.join("gcloud.log").display()
        ),
    )
    .unwrap();
    path
}

fn lnk_gcp(dir: &Path, gcloud: &Path) -> Command {
    let mut command = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-gcp"), &dir.join("home"));
    command
        .env("HOME", dir.join("home"))
        // CI runners set it, and it would put the settings outside HOME.
        .env_remove("XDG_CONFIG_HOME")
        .env("LNK_SERVICES", "off")
        .env("LNK_GCLOUD", gcloud)
        .env("PATH", "/usr/bin:/bin");
    command
}

fn call(dir: &Path, gcloud: &Path, command: &str, request: serde_json::Value) -> serde_json::Value {
    let mut child = lnk_gcp(dir, gcloud)
        // The shell's own settings must not redirect a box's calls.
        .env("CLOUDSDK_CORE_PROJECT", "someone-elses")
        .env(
            "CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE",
            "/shell/other-account.json",
        )
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

/// A T4 attached to an N1 machine: listed by `sizes` from the zone's
/// accelerator types, and made from its size alone (as a pinned one is),
/// with NVIDIA's driver.
#[test]
fn an_n1_box_with_a_t4_attached() {
    let dir = TempDir::new("adapter-t4");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let gcloud = fake(&dir);
    let account = serde_json::json!({
        "name": "gcp", "kind": "gcp", "place": "us-central1-a", "project": "my-project",
        "env": {}
    });
    let sizes = call(
        &dir,
        &gcloud,
        "sizes",
        serde_json::json!({"account": account, "place": "us-central1-a"}),
    );
    let t4: Vec<_> = sizes["sizes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["gpu_model"] == "nvidia-t4")
        .map(|s| (s["size"].as_str().unwrap(), s["gpus"].as_u64().unwrap()))
        .collect();
    assert_eq!(
        t4,
        [
            ("n1-standard-4.nvidia-tesla-t4.1", 1),
            ("n1-standard-4.nvidia-tesla-t4.2", 2),
            ("n1-standard-4.nvidia-tesla-t4.4", 4)
        ]
    );
    let spec = serde_json::json!({
        "name": "t4-1", "size": "n1-standard-4.nvidia-tesla-t4.2", "user_data": "",
        "public_key": "ssh-ed25519 AAAA", "tags": {"lnk-box": "t4-1"}
    });
    call(
        &dir,
        &gcloud,
        "launch",
        serde_json::json!({"account": account, "spec": spec}),
    );
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    let create = log
        .lines()
        .find(|l| l.starts_with("compute instances create lnk-t4-1"))
        .unwrap();
    for part in [
        "--machine-type n1-standard-4 ",
        "--image-family ubuntu-accelerator-2404-amd64-with-nvidia-550",
        "--accelerator type=nvidia-tesla-t4,count=2 --maintenance-policy TERMINATE",
    ] {
        assert!(create.contains(part), "{part} in {create}");
    }
}

/// A spot box with GPUs, in a region: the accelerator image, a spot VM
/// stopping when taken back, its disk, and the region's first zone up.
#[test]
fn a_spot_box_with_gpus_in_a_region() {
    let dir = TempDir::new("adapter-spot");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let gcloud = fake(&dir);
    let account = serde_json::json!({
        "name": "gcp", "kind": "gcp", "place": "us-central1-a", "project": "my-project",
        "env": {}
    });
    let spec = serde_json::json!({
        "name": "gpu-1", "size": "g2-standard-4", "user_data": "", "public_key": "ssh-ed25519 AAAA",
        "os": "ubuntu-22.04", "disk_gb": 100, "disk_type": "fast", "spot": true,
        "gpus": 1, "gpu_model": "nvidia-l4",
        "tags": {"lnk-box": "gpu-1", "team": "research"}
    });
    let defaults = call(
        &dir,
        &gcloud,
        "defaults",
        serde_json::json!({"account": account, "place": "europe-west2"}),
    );
    assert_eq!(defaults["place"], "europe-west2-b");
    call(
        &dir,
        &gcloud,
        "launch",
        serde_json::json!({"account": account, "place": "europe-west2", "spec": spec}),
    );
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    let create = log
        .lines()
        .find(|l| l.starts_with("compute instances create lnk-gpu-1"))
        .unwrap();
    for part in [
        "--zone europe-west2-b --machine-type g2-standard-4",
        "--image-family ubuntu-accelerator-2204-amd64-with-nvidia-550 --image-project ubuntu-os-accelerator-images",
        "--boot-disk-size 100GB --boot-disk-type pd-ssd",
        "--provisioning-model SPOT --instance-termination-action STOP",
        "--maintenance-policy TERMINATE",
        // Your tags are labels; the one network tag is Link's.
        "--tags lnk-box --labels lnk-box=gpu-1,team=research",
    ] {
        assert!(create.contains(part), "{part} in {create}");
    }
    // As Google Cloud says it is, for `lnk box spec`.
    let described = call(
        &dir,
        &gcloud,
        "describe",
        serde_json::json!({"account": account, "place": "europe-west2-b", "id": "lnk-gpu-1"}),
    );
    assert_eq!(
        described["boxes"][0],
        serde_json::json!({
            "id": "lnk-gpu-1", "name": "gpu-1", "machine_id": "", "place": "europe-west2-b",
            "state": "terminated", "size": "g2-standard-4", "spot": true, "disk_gb": 100,
            "disk_type": "fast", "os": "ubuntu-22.04", "arch": "x86_64", "tags":{"lnk-box":"gpu-1"}
        })
    );
    // A GPU box at the default disk: the accelerator image's is larger,
    // so it gets that, said.
    let mut small = spec.clone();
    small["name"] = "small".into();
    small.as_object_mut().unwrap().remove("disk_gb");
    let output = lnk_gcp(&dir, &gcloud)
        .arg("launch")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(
                serde_json::json!({"account": account, "spec": small})
                    .to_string()
                    .as_bytes(),
            )?;
            child.wait_with_output()
        })
        .unwrap();
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{said}");
    assert!(
        said.contains("Its image needs a disk of 50 GB at least: the box gets 50 GB, not 20."),
        "{said}"
    );
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    let create = log
        .lines()
        .find(|l| l.starts_with("compute instances create lnk-small"))
        .unwrap();
    assert!(create.contains("--boot-disk-size 50GB"), "{create}");
    assert!(log.contains(
        "compute images describe-from-family ubuntu-accelerator-2204-amd64-with-nvidia-550 --project ubuntu-os-accelerator-images --format json"
    ));
    // Refused: what Google said, then where to raise the quota.
    let refused = lnk_gcp(&dir, &gcloud)
        .arg("launch")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            let mut spec = spec.clone();
            spec["name"] = "refused".into();
            child.stdin.take().unwrap().write_all(
                serde_json::json!({"account": account, "spec": spec})
                    .to_string()
                    .as_bytes(),
            )?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.contains("GPUS_ALL_REGIONS"), "{said}");
    assert!(said.contains("IAM & Admin, Quotas"), "{said}");
}

#[test]
fn an_account_is_gclouds_login_and_a_box_an_instance() {
    let dir = TempDir::new("adapter-all");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let gcloud = fake(&dir);
    let output = lnk_gcp(&dir, &gcloud)
        .args(["connect", "--name", "gcp"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let account: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(account["project"], "my-project");
    assert_eq!(account["place"], "us-central1-a");
    assert_eq!(account["identity"], "Google Cloud as me@example.com");
    assert_eq!(
        account["settings"]["CLOUDSDK_CORE_ACCOUNT"],
        "me@example.com"
    );
    // The login's application default credentials, made once and kept as
    // the account's own file: what its buckets act as.
    let file = dir.join("home/.lnk/safe/gcp/credentials/gcp.json");
    assert_eq!(
        account["settings"]["GOOGLE_APPLICATION_CREDENTIALS"],
        file.display().to_string()
    );
    assert_eq!(
        account["settings"]["CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE"],
        file.display().to_string()
    );
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("authorized_user")
    );
    let storage = call(
        &dir,
        &gcloud,
        "storage",
        serde_json::json!({"account": account}),
    );
    assert_eq!(
        storage,
        serde_json::json!({
            "credential": {"type": "google cloud storage", "env_auth": "true"},
            "env": {"GOOGLE_APPLICATION_CREDENTIALS": file.display().to_string()}
        })
    );

    let spec = serde_json::json!({
        "name": "work", "size": "e2-small", "user_data": "#cloud-config\n",
        "public_key": "ssh-ed25519 AAAAC3Nza lnk-box"
    });
    // A spec from an lnk box with IDs: named after the ID, labelled.
    let tagged = call(
        &dir,
        &gcloud,
        "launch",
        serde_json::json!({"account": account, "spec": {
            "name": "gcp-1", "id": "k3v9q2x7m1ab", "size": "e2-small", "user_data": "",
            "tags": {"lnk-box": "gcp-1", "lnk-machine": "k3v9q2x7m1ab", "lnk-owner": "github-7"}
        }}),
    );
    assert_eq!(tagged["id"], "lnk-k3v9q2x7m1ab");
    let launched = call(
        &dir,
        &gcloud,
        "launch",
        serde_json::json!({"account": account, "spec": spec}),
    );
    assert_eq!(launched["id"], "lnk-work");
    let running = call(
        &dir,
        &gcloud,
        "wait",
        serde_json::json!({"account": account, "id": "lnk-work"}),
    );
    assert_eq!(running["ip"], "198.51.100.7");
    let keys = call(
        &dir,
        &gcloud,
        "hostkeys",
        serde_json::json!({"account": account, "id": "lnk-work"}),
    );
    assert_eq!(
        keys["host_keys"],
        serde_json::json!([
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGx1pZlbXq0WyU1b4v5r0Ke+ZtrQ2mX5d9J8lJk3aB7c"
        ])
    );
    let state = call(
        &dir,
        &gcloud,
        "state",
        serde_json::json!({"account": account, "id": "lnk-work"}),
    );
    assert_eq!(state["state"], "running");
    call(
        &dir,
        &gcloud,
        "remove",
        serde_json::json!({"account": account, "id": "lnk-work"}),
    );
    call(
        &dir,
        &gcloud,
        "cleanup",
        serde_json::json!({"account": account}),
    );

    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    assert!(log.contains("--labels lnk-box=gcp-1,lnk-machine=k3v9q2x7m1ab,lnk-owner=github-7"));
    let create = log
        .lines()
        .find(|l| l.starts_with("compute instances create lnk-work"))
        .unwrap();
    for part in [
        "lnk-work --zone us-central1-a --machine-type e2-small",
        "--image-family ubuntu-2404-lts-amd64 --image-project ubuntu-os-cloud",
        "--tags lnk-box",
        "--labels lnk-box=work",
        "--metadata ssh-keys=ubuntu:ssh-ed25519 AAAAC3Nza lnk-box,enable-oslogin=FALSE,block-project-ssh-keys=TRUE",
        // No identity for programs on the box to take.
        "--no-service-account --no-scopes",
        // Today's disk, from an lnk box before specs.
        "--boot-disk-size 20GB --boot-disk-type pd-balanced",
    ] {
        assert!(create.contains(part), "{part} in {create}");
    }
    assert!(!create.contains("SPOT") && !create.contains("--maintenance-policy"));
    // Today's lnk box gives no key: the box trusts Link's certificates.
    let today = log
        .lines()
        .find(|l| l.starts_with("compute instances create lnk-k3v9q2x7m1ab"))
        .unwrap();
    assert!(
        today.contains("--metadata enable-oslogin=FALSE,block-project-ssh-keys=TRUE"),
        "{today}"
    );
    assert!(!today.contains("ubuntu:"), "{today}");
    assert!(log.contains("compute firewall-rules create lnk-box-ssh --network default --action allow --rules tcp:22 --source-ranges 0.0.0.0/0 --priority 1000 --target-tags lnk-box"));
    // The default network lets in every port from inside it: all else
    // is refused ahead of its rules.
    assert!(log.contains("compute firewall-rules create lnk-box-deny --network default --action deny --rules all --source-ranges 0.0.0.0/0 --priority 65000 --target-tags lnk-box"));
    assert!(log.contains("compute firewall-rules delete lnk-box-ssh --quiet"));
    assert!(log.contains("compute firewall-rules delete lnk-box-deny --quiet"));
    assert!(log.contains(
        "compute instances delete lnk-work --zone us-central1-a --delete-disks boot --quiet"
    ));
    assert!(log.contains("CLOUDSDK_CORE_PROJECT=my-project"));
    assert!(!log.contains("someone-elses"));
}

mod credentials;
mod machine;

/// A box found from another computer: by its owner label, its key merged
/// in, its host keys kept on it and read back, its labels added to.
#[test]
fn boxes_are_found_by_their_labels_and_let_another_computer_in() {
    let dir = TempDir::new("adapter-found");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let gcloud = fake(&dir);
    let account = serde_json::json!({
        "name": "gcp", "kind": "gcp", "place": "us-central1-a", "project": "my-project",
        "env": {"CLOUDSDK_CORE_ACCOUNT": "me@example.com"}
    });
    let found = call(
        &dir,
        &gcloud,
        "list",
        serde_json::json!({"account": account, "owner": "github-7"}),
    );
    assert_eq!(
        found["boxes"],
        serde_json::json!([{
            "id": "lnk-k3v9q2x7m1ab", "name": "gcp-1", "machine_id": "k3v9q2x7m1ab",
            "owner": "github-7", "place": "europe-west1-b", "state": "terminated",
            "size": "e2-small", "tags":{"lnk-box":"gcp-1","lnk-machine":"k3v9q2x7m1ab","lnk-owner":"github-7"}
        }])
    );
    // With no owner, every owner's boxes, each saying whose.
    let every = call(
        &dir,
        &gcloud,
        "list",
        serde_json::json!({"account": account}),
    );
    let owners: Vec<_> = every["boxes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (
                b["machine_id"].as_str().unwrap(),
                b["owner"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        owners,
        [("k3v9q2x7m1ab", "github-7"), ("p8r2c4n6w0zt", "acct-9")]
    );
    // An owner that isn't a label value never reaches the filter.
    let bad = lnk_gcp(&dir, &gcloud)
        .arg("list")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(
                serde_json::json!({"account": account, "owner": "x OR labels.a:*"})
                    .to_string()
                    .as_bytes(),
            )?;
            child.wait()
        })
        .unwrap();
    assert!(!bad.success());

    let key = "ssh-ed25519 AAAAnew lnk k3v9q2x7m1ab danas-mbp";
    call(
        &dir,
        &gcloud,
        "add-key",
        serde_json::json!({"account": account, "id": "lnk-work", "public_key": key}),
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("added-ssh-keys")).unwrap(),
        format!(
            "ubuntu:ssh-ed25519 AAAAold lnk abc123 other-mac\n\
             ubuntu:ssh-ed25519 AAAAmine dana@laptop\nubuntu:{key}"
        )
    );
    // Link's own keys go once the box trusts certificates; yours stay.
    call(
        &dir,
        &gcloud,
        "remove-keys",
        serde_json::json!({"account": account, "id": "lnk-work"}),
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("added-ssh-keys")).unwrap(),
        "ubuntu:ssh-ed25519 AAAAmine dana@laptop"
    );
    let pinned = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGx1pZlbXq0WyU1b4v5r0Ke+ZtrQ2mX5d9J8lJk3aB7c";
    call(
        &dir,
        &gcloud,
        "save-hostkeys",
        serde_json::json!({"account": account, "id": "lnk-work", "host_keys": [pinned]}),
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("added-lnk-host-keys")).unwrap(),
        pinned
    );
    // Kept keys come before the console's.
    let kept = call(
        &dir,
        &gcloud,
        "hostkeys",
        serde_json::json!({"account": account, "id": "lnk-kept"}),
    );
    assert_eq!(
        kept["host_keys"],
        serde_json::json!([
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKEPTkeptKEPTkeptKEPTkeptKEPTkeptKEPTkeptKEPT"
        ])
    );
    call(
        &dir,
        &gcloud,
        "tag",
        serde_json::json!({"account": account, "id": "lnk-work",
            "tags": {"lnk-box": "gcp-1", "lnk-owner": "github-7"}}),
    );
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    assert!(
        log.contains(
            "compute instances add-labels lnk-work --zone us-central1-a --labels lnk-box=gcp-1,lnk-owner=github-7"
        ),
        "{log}"
    );
}

#[test]
fn an_agents_bucket_is_labelled_found_and_keyed_alone() {
    let dir = TempDir::new("adapter-buckets");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let gcloud = fake(&dir);
    let account = serde_json::json!({
        "name": "gcp", "kind": "gcp", "place": "us-central1-a", "project": "my-project",
        "env": {"CLOUDSDK_CORE_ACCOUNT": "me@example.com"}
    });
    let bucket = "lnk-bucket-a1b2c3d4e5f6";
    call(
        &dir,
        &gcloud,
        "label",
        serde_json::json!({"account": account, "bucket": bucket, "labels": {"lnk-folder-name": "moltbot-2"}}),
    );
    let found = call(
        &dir,
        &gcloud,
        "buckets",
        serde_json::json!({"account": account, "labels": {"lnk-folder": "p8r2c4n6w0zt"}}),
    );
    assert_eq!(
        found["buckets"],
        serde_json::json!([{"bucket": bucket, "place": "europe-west1",
            "labels": {"lnk-folder": "p8r2c4n6w0zt"}}])
    );
    let credential = call(
        &dir,
        &gcloud,
        "bucket-credential",
        serde_json::json!({"account": account, "bucket": bucket}),
    );
    assert_eq!(credential["credential"]["type"], "google cloud storage");
    assert_eq!(credential["credential"]["bucket_policy_only"], "true");
    // gcloud indents the key over many lines; a storage setting is one.
    let key = credential["credential"]["service_account_credentials"]
        .as_str()
        .unwrap();
    assert!(!key.contains(['\n', '\r']), "{key}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(key).unwrap(),
        serde_json::json!({"type": "service_account",
            "private_key": "-----BEGIN-----\nsecret\n-----END-----\n"})
    );
    // The key file is gone once read.
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    let path = log
        .lines()
        .find_map(|l| l.strip_prefix("iam service-accounts keys create "))
        .and_then(|l| l.split_whitespace().next())
        .unwrap()
        .to_string();
    assert!(!Path::new(&path).exists(), "{path}");
    // Gone already is gone.
    call(
        &dir,
        &gcloud,
        "revoke-key",
        serde_json::json!({"account": account, "bucket": bucket}),
    );
}

/// A service account, by its key: gcloud and buckets both act as it, the
/// key kept as the account's own file, and gone when it's forgotten.
#[test]
fn a_service_account_is_its_key_for_boxes_and_buckets() {
    let dir = TempDir::new("adapter-key");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let gcloud = fake(&dir);
    let key = dir.join("key.json");
    std::fs::write(
        &key,
        r#"{"type": "service_account", "client_email": "ci@my-project.iam.gserviceaccount.com", "project_id": "my-project", "private_key": "secret"}"#,
    )
    .unwrap();
    let output = lnk_gcp(&dir, &gcloud)
        .args(["connect", "--name", "ci", "--credential-file"])
        .arg(&key)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let account: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        account["identity"],
        "Google Cloud as ci@my-project.iam.gserviceaccount.com"
    );
    assert_eq!(account["project"], "my-project");
    let file = dir.join("home/.lnk/safe/gcp/credentials/ci.json");
    let path = file.display().to_string();
    assert_eq!(account["settings"]["GOOGLE_APPLICATION_CREDENTIALS"], path);
    assert_eq!(
        account["settings"]["CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE"],
        path
    );
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    // gcloud checked the project as the key, not its own login.
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    assert!(log.contains(&format!("CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE={path}")));
    assert!(!log.contains("auth application-default login"));
    let storage = call(
        &dir,
        &gcloud,
        "storage",
        serde_json::json!({"account": account}),
    );
    assert_eq!(
        storage,
        serde_json::json!({"credential": {"type": "google cloud storage", "service_account_file": path}})
    );
    call(
        &dir,
        &gcloud,
        "disconnect",
        serde_json::json!({"account": account}),
    );
    assert!(!file.exists());
    // Forgotten, it can't sign in until connected again.
    let gone = lnk_gcp(&dir, &gcloud)
        .arg("storage")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(
                serde_json::json!({"account": account})
                    .to_string()
                    .as_bytes(),
            )?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(!gone.status.success());
    assert!(String::from_utf8_lossy(&gone.stderr).contains("lnk cloud connect gcp --name ci"));
}

/// gcloud signed in as a service account hands out no key: its key is
/// asked for instead.
#[test]
fn gclouds_service_account_login_needs_its_key() {
    let dir = TempDir::new("adapter-robot");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let gcloud = fake(&dir);
    let output = lnk_gcp(&dir, &gcloud)
        .env("FAKE_ACCOUNT", "ci@my-project.iam.gserviceaccount.com")
        .args(["connect", "--name", "gcp"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--credential-file"));
}

/// `lnk-gcp tunnel` is a box's SSH over IAP, as ssh's ProxyCommand runs
/// it: `gcloud compute start-iap-tunnel` to the box's port 22, on this
/// program's own stdin and stdout, as the account `lnk cloud env` names
/// and in its project.
#[test]
fn a_tunnel_carries_ssh_over_iap() {
    let dir = TempDir::new("adapter-tunnel");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let gcloud = fake(&dir);
    let lnk = dir.join("lnk");
    link_test_kit::write_program(
        &lnk,
        r#"#!/bin/sh
case "$*" in
  "cloud env gcp --json") echo '{"name": "gcp", "kind": "gcp", "place": "us-central1-a", "project": "my-project", "env": {"CLOUDSDK_CONFIG": "/somewhere"}}' ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    let mut child = lnk_gcp(&dir, &gcloud)
        .env("LNK_EXE", &lnk)
        .env("CLOUDSDK_CORE_PROJECT", "someone-elses")
        .args([
            "tunnel",
            "--cloud",
            "gcp",
            "--zone",
            "europe-west1-b",
            "--id",
            "lnk-k3v9q2x7m1ab",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"SSH-2.0-OpenSSH_9.6\r\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"SSH-2.0-OpenSSH_9.6\r\n");
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    assert_eq!(
        log.lines().next().unwrap(),
        "compute start-iap-tunnel lnk-k3v9q2x7m1ab 22 --listen-on-stdin --zone=europe-west1-b --verbosity warning"
    );
    assert!(log.contains("CLOUDSDK_CORE_PROJECT=my-project"), "{log}");
    assert!(!log.contains("someone-elses"), "{log}");
}

#[test]
fn a_disposable_job_has_only_its_own_deletion_and_a_cloud_deadline() {
    let dir = TempDir::new("adapter-job-end");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let gcloud = fake(&dir);
    let account = serde_json::json!({"name":"gcp", "kind":"gcp", "place":"us-central1-a", "project":"my-project", "env":{}});
    // A box's first boot, its deadline in, as the box module writes it.
    let (files, commands) = link_cloud_contract::first_boot::deadline(14500);
    let boot = link_cloud_contract::first_boot::with(
        "#cloud-config\nwrite_files:\n  - path: /etc/lnk/kind.toml\n    owner: root:root\n    \
         permissions: '0644'\n    content: |\n      kind = \"box\"\nruncmd:\n  - [chmod, '0755', /etc/lnk]\n",
        &files,
        &commands,
    );
    call(
        &dir,
        &gcloud,
        "launch",
        serde_json::json!({"account":account,"spec":{
            "name":"job", "id":"k3v9q2x7m1ab", "size":"e2-small", "public_key":"ssh-ed25519 AAAA",
            "user_data":boot, "deadline":"4h", "self_remove":true
        }}),
    );
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    assert!(
        log.contains("--permissions=compute.instances.delete --stage=GA"),
        "{log}"
    );
    assert!(
        log.contains(
            "resource.name == 'projects/my-project/zones/us-central1-a/instances/lnk-k3v9q2x7m1ab'"
        ),
        "{log}"
    );
    assert!(
        log.contains("--service-account lnk-job-k3v9q2x7m1ab@my-project.iam.gserviceaccount.com")
    );
    // The grant IAM refused while the account was new was tried again.
    assert_eq!(
        log.matches("projects add-iam-policy-binding my-project")
            .count(),
        2,
        "{log}"
    );
    assert!(log.contains("--max-run-duration 14400s --instance-termination-action=DELETE"));
    let init = std::fs::read_to_string(dir.join("first-boot")).unwrap();
    assert!(init.contains("/etc/lnk/end.json"));
    assert!(
        init.contains(
            "compute/v1/projects/my-project/zones/us-central1-a/instances/lnk-k3v9q2x7m1ab"
        )
    );
    assert!(!init.contains("/sbin/poweroff"));
    assert!(!init.contains("lnk-deadline"), "{init}");
    // Every list item two spaces in, as the box module's: one out of step
    // and cloud-init reads none of the file.
    let items: Vec<&str> = init
        .lines()
        .filter(|l| l.trim_start().starts_with("- "))
        .collect();
    assert_eq!(items.len(), 3, "{init}");
    assert!(items.iter().all(|l| l.starts_with("  - ")), "{init}");
    let signed = call(
        &dir,
        &gcloud,
        "upload-once",
        serde_json::json!({"account":account,"bucket":"results","object":"job/output.tar","expires":"4h"}),
    );
    assert_eq!(
        signed["upload"]["headers"]["x-goog-if-generation-match"],
        "0"
    );
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    assert!(log.contains("--http-verb=PUT --headers=x-goog-if-generation-match=0"));
    // A role already there, made by the project's setup, is read and
    // never made again, so an account that may not make roles can launch.
    call(
        &dir,
        &gcloud,
        "launch",
        serde_json::json!({"account":account,"spec":{
            "name":"job2", "id":"m4n5p6q7r8st", "size":"e2-small", "public_key":"ssh-ed25519 AAAA",
            "user_data":boot, "deadline":"4h", "self_remove":true
        }}),
    );
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    assert_eq!(
        log.matches("iam roles create lnkBoxEnd").count(),
        1,
        "{log}"
    );
}

/// A credential override authenticates gcloud, but sign-url also needs
/// the service-account file explicitly to select its signing implementation.
#[test]
fn result_uploads_sign_with_the_connected_service_account_file() {
    let dir = TempDir::new("result-signing-file");
    let gcloud = fake(&dir);
    let credentials = dir.join("service account.json");
    std::fs::write(&credentials, r#"{"type":"service_account"}"#).unwrap();
    let account = serde_json::json!({
        "name":"gcp", "kind":"gcp", "place":"us-central1-a", "project":"my-project",
        "env":{"CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE":credentials,
               "GOOGLE_APPLICATION_CREDENTIALS":credentials}
    });
    let signed = call(
        &dir,
        &gcloud,
        "upload-once",
        serde_json::json!({"account":account,"bucket":"results","object":"job/output.tar","expires":"5m"}),
    );
    assert_eq!(
        signed["upload"]["headers"]["x-goog-if-generation-match"],
        "0"
    );
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    assert!(
        log.contains(&format!("--private-key-file={}", credentials.display())),
        "{log}"
    );
    assert!(!log.contains("/shell/other-account.json"), "{log}");
    assert!(
        log.contains("--duration=300s --http-verb=PUT --headers=x-goog-if-generation-match=0"),
        "{log}"
    );
}

#[test]
fn attachments_survive_replacement_and_repeated_removal() {
    let dir = TempDir::new("attached");
    let gcloud = fake(&dir);
    let account = serde_json::json!({"name":"gcp","kind":"gcp","place":"us-central1-a","project":"my-project","env":{}});
    let spec = serde_json::json!({"name":"service","size":"e2-small","user_data":"","public_key":"ssh-ed25519 AAAA", "attach":{"network":"control","identity":"service","address":"fixed","disks":["data"]}});
    for _ in 0..2 {
        call(
            &dir,
            &gcloud,
            "defaults",
            serde_json::json!({"account":account,"spec":spec}),
        );
        let created = call(
            &dir,
            &gcloud,
            "launch",
            serde_json::json!({"account":account,"spec":spec}),
        );
        for _ in 0..2 {
            call(
                &dir,
                &gcloud,
                "remove",
                serde_json::json!({"account":account,"id":created["id"]}),
            );
        }
    }
    let log = std::fs::read_to_string(dir.join("gcloud.log")).unwrap();
    assert!(
        log.contains("--network control")
            && log.contains("--service-account service@my-project.iam.gserviceaccount.com")
            && log.contains("--address fixed"),
        "{log}"
    );
    assert!(
        log.contains("name=data,boot=no,auto-delete=no,mode=rw"),
        "{log}"
    );
    assert!(log.contains("--delete-disks boot"), "{log}");
    assert!(
        !log.contains("--delete-disks all")
            && !log.contains("firewall-rules create")
            && !log.contains("addresses delete")
            && !log.contains("disks delete"),
        "{log}"
    );
}
