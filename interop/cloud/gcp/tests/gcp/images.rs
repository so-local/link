//! `lnk-gcp`'s box images with a fake `gcloud`: an image of a box's boot
//! disk, found by its labels, a box made from it, and the image deleted.
//! Only the project's own, labelled as Link's, are touched.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use link_test_kit::TempDir;

const IMAGE: &str = r#"{"name": "lnk-web-ready", "status": "READY", "diskSizeGb": "30", "architecture": "X86_64", "creationTimestamp": "2026-10-01T03:00:00.000-07:00", "labels": {"lnk-image": "web-ready", "lnk-hash": "3f2a9c", "lnk-owner": "github-7"}}"#;

fn gcloud() -> String {
    format!(
        r#"case "$*" in
  "compute instances describe lnk-k3v9q2x7m1ab "*) echo '{{"name": "lnk-k3v9q2x7m1ab", "status": "TERMINATED", "disks": [{{"boot": false, "source": "https://x/disks/data"}}, {{"boot": true, "source": "https://x/zones/europe-west1-b/disks/lnk-k3v9q2x7m1ab"}}]}}' ;;
  "compute images create "*) echo '[]' ;;
  "compute images describe lnk-web-ready --format json") echo '{IMAGE}' ;;
  "compute images describe lnk-pending --format json") echo '{{"name": "lnk-pending", "status": "PENDING"}}' ;;
  "compute images describe theirs --format json") echo '{{"name": "theirs", "status": "READY", "labels": {{"lnk-owner": "github-7"}}}}' ;;
  "compute images describe "*) echo 'ERROR: The resource was not found' >&2; exit 1 ;;
  "compute images list "*) echo '[{IMAGE}, {{"name": "unlabelled", "status": "READY", "labels": {{"lnk-owner": "github-7"}}}}]' ;;
  "compute images delete "*) ;;
  "compute firewall-rules describe "*) echo '{{}}' ;;
  "compute instances create "*) echo '[]' ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac"#
    )
}

fn fake(dir: &Path) -> PathBuf {
    let path = dir.join("gcloud");
    link_test_kit::write_program(
        &path,
        format!(
            "#!/bin/sh\necho \"$*\" >> {log}\n{script}\n",
            log = dir.join("gcloud.log").display(),
            script = gcloud()
        ),
    )
    .unwrap();
    path
}

/// `lnk-gcp <command>` with `request`: its reply, or its error.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn call(
    dir: &Path,
    command: &str,
    request: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lnk-gcp"))
        .env("HOME", dir.join("home"))
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(&dir.join("home")))
        .env("LNK_GCLOUD", dir.join("gcloud"))
        .env("PATH", "/usr/bin:/bin")
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
    serde_json::json!({"name": "gcp", "kind": "gcp", "place": "europe-west1-b",
        "project": "my-project", "env": {}})
}

#[test]
fn image_inventory_without_owner_preserves_unlabelled_native_ids() {
    let dir = TempDir::new("images-all-owners");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    fake(&dir);
    let reply = call(&dir, "image-list", serde_json::json!({"account":account()})).unwrap();
    assert_eq!(reply["images"].as_array().unwrap().len(), 2);
    assert!(
        reply["images"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == "unlabelled")
    );
    assert!(!log(&dir).contains("--filter"));
    assert!(log(&dir).contains("--no-standard-images"));
}

fn log(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("gcloud.log")).unwrap_or_default()
}

#[test]
fn a_box_is_saved_as_an_image_of_its_boot_disk() {
    let dir = TempDir::new("images-save");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    fake(&dir);
    let saved = call(
        &dir,
        "image-save",
        serde_json::json!({"account": account(), "place": "europe-west1-b",
            "id": "lnk-k3v9q2x7m1ab", "image": "web-ready",
            "tags": {"lnk-image": "web-ready", "lnk-hash": "3f2a9c", "lnk-owner": "github-7"}}),
    )
    .unwrap();
    assert_eq!(
        saved["images"][0],
        serde_json::json!({"id": "lnk-web-ready", "name": "web-ready", "place": "",
            "state": "ready", "disk_gb": 30, "arch": "x86_64",
            "created": "2026-10-01T03:00:00.000-07:00", "hash": "3f2a9c"})
    );
    let log = log(&dir);
    assert!(
        log.contains(
            "compute images create lnk-web-ready --source-disk lnk-k3v9q2x7m1ab \
             --source-disk-zone europe-west1-b --labels lnk-hash=3f2a9c,lnk-image=web-ready,lnk-owner=github-7"
        ),
        "{log}"
    );
    // Never --force: the box is stopped first.
    assert!(!log.contains("--force"), "{log}");
    let bad = call(
        &dir,
        "image-save",
        serde_json::json!({"account": account(), "id": "lnk-k3v9q2x7m1ab", "image": "Web Ready",
            "tags": {"lnk-image": "x", "lnk-owner": "github-7"}}),
    );
    assert!(bad.unwrap_err().contains("can't name an image"));
    assert_eq!(self::log(&dir).matches("compute images create").count(), 1);
}

#[test]
fn images_are_found_by_their_labels_and_a_box_made_from_one() {
    let dir = TempDir::new("images-list");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    fake(&dir);
    let listed = call(
        &dir,
        "image-list",
        serde_json::json!({"account": account(), "owner": "github-7"}),
    )
    .unwrap();
    // Only those labelled as images too.
    let names: Vec<_> = listed["images"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["web-ready"]);
    assert!(log(&dir).contains(
        "compute images list --no-standard-images --filter labels.lnk-owner=github-7 --format json"
    ));
    assert!(
        call(
            &dir,
            "image-list",
            serde_json::json!({"account": account(), "owner": "x OR labels.a:*"}),
        )
        .is_err()
    );

    let spec = serde_json::json!({"name": "web-1", "id": "p8r2c4n6w0zt", "size": "e2-small",
        "user_data": "", "public_key": "ssh-ed25519 AAAAC3Nza lnk-box", "disk_gb": 30});
    let made = call(
        &dir,
        "launch",
        serde_json::json!({"account": account(), "spec": spec, "image": "lnk-web-ready"}),
    )
    .unwrap();
    assert_eq!(made["id"], "lnk-p8r2c4n6w0zt");
    let log = log(&dir);
    let create = log
        .lines()
        .find(|l| l.starts_with("compute instances create "))
        .unwrap();
    assert!(create.contains("--image lnk-web-ready "), "{create}");
    assert!(!create.contains("--image-family"), "{create}");
    assert!(create.contains("--boot-disk-size 30GB"), "{create}");
    // One not ready, or not there, is refused before anything's made.
    for (image, why) in [
        ("lnk-pending", "is pending, not ready"),
        ("lnk-gone", "no image lnk-gone in this project"),
        ("--image-project=x", "isn't an image's name"),
    ] {
        let refused = call(
            &dir,
            "launch",
            serde_json::json!({"account": account(), "spec": spec, "image": image}),
        );
        assert!(refused.unwrap_err().contains(why), "{image}");
    }
    assert_eq!(
        self::log(&dir).matches("compute instances create").count(),
        1
    );
}

#[test]
fn an_image_goes_only_if_its_links() {
    let dir = TempDir::new("images-remove");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    fake(&dir);
    call(
        &dir,
        "image-remove",
        serde_json::json!({"account": account(), "image": "lnk-web-ready", "owner": "github-7"}),
    )
    .unwrap();
    assert!(log(&dir).contains("compute images delete lnk-web-ready --quiet"));
    for (image, owner) in [("lnk-web-ready", "github-8"), ("theirs", "github-7")] {
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
    assert_eq!(log(&dir).matches("compute images delete").count(), 1);
    // Gone already is done.
    call(
        &dir,
        "image-remove",
        serde_json::json!({"account": account(), "image": "lnk-gone", "owner": "github-7"}),
    )
    .unwrap();
}
