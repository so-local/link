//! What each command does, above the engine: which files, which clouds,
//! what the manifest records, and when to refuse. Each command that
//! deletes is split in two, a plan (checked, nothing changed yet) and
//! applying it, so the command can ask in between.

use std::path::Path;

use anyhow::bail;
use serde::Serialize;

use crate::clouds::{Cloud, is_archive, needs_restore};
use crate::engine::{Download, Engine, Found, Proof, Sending, Upload, Written};
use crate::files::{self, Local, matches};
use crate::manifest::{Copy, Entry, Manifest, Proven};

mod cleaning;
mod listing;
mod pulling;
mod pushing;
mod removing;
pub use cleaning::{CleanPlan, apply_clean, plan_clean};
pub use listing::{Here, Mark, Row, only_in, refresh, tree};
pub use pulling::{PullPlan, apply_pull, plan_pull};
use pushing::record;
pub use pushing::{Checked, Push, locals, push};
pub use removing::{RemovePlan, apply_remove, plan_remove};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clouds::Kind;
    use crate::engine::fake::Fake;

    struct Setup {
        root: link_test_kit::TempDir,
        engine: Fake,
        manifest: Manifest,
        clouds: Vec<Cloud>,
    }

    fn cloud(name: &str, kind: Kind) -> Cloud {
        Cloud {
            name: name.into(),
            kind,
            folder: "personal".into(),
            bucket: "b".into(),
            region: None,
            path: None,
            backend: None,
            encrypted: false,
        }
    }

    fn setup(name: &str) -> Setup {
        let root = link_test_kit::TempDir::new(&format!("ops-{name}"));
        std::fs::create_dir_all(root.join("photos/2019")).unwrap();
        std::fs::write(root.join("photos/2019/a.jpg"), "aaa").unwrap();
        std::fs::write(root.join("photos/2019/b.jpg"), "bbbb").unwrap();
        std::fs::write(root.join("notes.txt"), "n").unwrap();
        Setup {
            root,
            engine: Fake::default(),
            manifest: Manifest::default(),
            clouds: vec![cloud("aws", Kind::Aws), cloud("drive", Kind::Drive)],
        }
    }

    fn paths(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    impl Setup {
        fn push(&mut self, p: &[&str]) -> Vec<Checked> {
            push(
                &self.engine,
                &self.root,
                &mut self.manifest,
                &self.clouds,
                &paths(p),
                &Push::default(),
            )
            .unwrap()
        }

        fn clean(&mut self, p: &[&str]) -> CleanPlan {
            plan_clean(
                &self.engine,
                &self.root,
                &mut self.manifest,
                &self.clouds,
                &paths(p),
            )
            .unwrap()
        }

        /// Cleans `p`, which must be safe.
        fn offload(&mut self, p: &[&str]) {
            let plan = self.clean(p);
            assert!(apply_clean(&self.root, &plan).unwrap().is_empty());
        }
    }

    /// A copy a clean proved isn't checked (read back, on S3) again by the
    /// next while it and the file here are as they were; written again,
    /// or written while it's checked, it is.
    #[test]
    fn clean_proves_a_copy_once() {
        let mut s = setup("proven");
        s.engine.uploader.borrow_mut().push("aws".into());
        s.push(&["photos"]);
        let checked = |s: &Setup| -> Vec<Vec<String>> {
            s.engine
                .to_delete
                .borrow()
                .iter()
                .filter(|(cloud, _)| cloud == "aws")
                .map(|(_, files)| files.clone())
                .collect()
        };
        // A dry run, then the clean: checked once, and listed again after
        // only when something was checked.
        assert!(s.clean(&["photos"]).safe());
        assert_eq!(s.engine.listed.borrow().len(), 2);
        assert!(s.clean(&["photos"]).safe());
        assert_eq!(s.engine.listed.borrow().len(), 3);
        let both = paths(&["photos/2019/a.jpg", "photos/2019/b.jpg"]);
        assert_eq!(checked(&s), [both.clone(), Vec::new()]);
        // Written again in the cloud: checked again.
        s.engine.put("aws", "photos/2019/a.jpg", b"aaa");
        assert!(s.clean(&["photos"]).safe());
        assert_eq!(checked(&s)[2], paths(&["photos/2019/a.jpg"]));
        // Changed here: checked again.
        std::fs::write(s.root.join("photos/2019/b.jpg"), "bbbb").unwrap();
        let file = std::fs::File::options()
            .write(true)
            .open(s.root.join("photos/2019/b.jpg"))
            .unwrap();
        file.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(7))
            .unwrap();
        assert!(s.clean(&["photos"]).safe());
        assert_eq!(checked(&s)[3], paths(&["photos/2019/b.jpg"]));
        // Written while it was checked: not taken, and not kept as proved.
        s.engine.put("aws", "photos/2019/a.jpg", b"aaa");
        *s.engine.written_while_checked.borrow_mut() =
            Some(("aws".into(), "photos/2019/a.jpg".into()));
        let plan = s.clean(&["photos"]);
        assert!(!plan.safe());
        let aws = plan.checks.iter().find(|c| c.cloud == "aws").unwrap();
        assert_eq!(aws.errors.len(), 1, "{:?}", aws.errors);
        assert!(aws.errors[0].1.contains("written while it was checked"));
        assert!(
            s.manifest.files["photos/2019/a.jpg"].clouds["aws"]
                .proven
                .is_none()
        );
        assert!(s.clean(&["photos"]).safe());
        // A cloud whose checksums are the storage's keeps no proof.
        assert!(
            s.manifest.files["photos/2019/a.jpg"].clouds["drive"]
                .proven
                .is_none()
        );
    }

    /// An upload in parts begun on a copy's name and still open, or one
    /// that can't be told, keeps the copy from being proved: it's checked
    /// again next time.
    #[test]
    fn an_open_upload_keeps_a_copy_from_being_proved() {
        let mut s = setup("pending");
        s.engine.uploader.borrow_mut().push("aws".into());
        s.push(&["photos"]);
        s.engine
            .pending
            .borrow_mut()
            .push(("aws".into(), "photos/2019/a.jpg".into()));
        assert!(s.clean(&["photos"]).safe());
        let proven = |s: &Setup, path: &str| s.manifest.files[path].clouds["aws"].proven.is_some();
        assert!(!proven(&s, "photos/2019/a.jpg"));
        assert!(proven(&s, "photos/2019/b.jpg"));
        s.engine.pending.borrow_mut().clear();
        *s.engine.pending_unknown.borrow_mut() = true;
        assert!(s.clean(&["photos"]).safe());
        assert!(!proven(&s, "photos/2019/a.jpg"));
        *s.engine.pending_unknown.borrow_mut() = false;
        assert!(s.clean(&["photos"]).safe());
        assert!(proven(&s, "photos/2019/a.jpg"));
    }

    #[test]
    fn push_records_checked_copies() {
        let mut s = setup("push");
        let reports = s.push(&["photos"]);
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().all(|r| r.all_same() && r.same.len() == 2));
        assert!(s.engine.has("aws", "photos/2019/a.jpg"));
        assert!(!s.engine.has("aws", "notes.txt"));
        let copy = &s.manifest.files["photos/2019/a.jpg"].clouds["drive"];
        assert!(copy.checked.is_some());
        let rows = tree(&s.root, &s.manifest, &s.clouds, "").unwrap();
        let notes = rows.iter().find(|r| r.path == "notes.txt").unwrap();
        assert_eq!(notes.here, Here::New);
        assert_eq!(notes.clouds[0].1, Mark::Missing);
        let a = rows.iter().find(|r| r.path == "photos/2019/a.jpg").unwrap();
        assert_eq!(a.here, Here::Here);
        assert_eq!(
            a.clouds,
            [
                ("aws".into(), Mark::Checked),
                ("drive".into(), Mark::Checked)
            ]
        );
    }

    #[test]
    fn push_skips_files_unchanged_since_checked() {
        let mut s = setup("skip");
        s.push(&["photos"]);
        // A copy gone behind Link's back isn't noticed by a push that
        // skips it, only by one with `all` (and by clean).
        s.engine.remove("aws", "photos/2019/a.jpg");
        let reports = s.push(&["photos"]);
        assert!(
            reports
                .iter()
                .all(|r| r.unchanged == 2 && r.same.is_empty() && r.all_same())
        );
        let reports = push(
            &s.engine,
            &s.root,
            &mut s.manifest,
            &s.clouds,
            &paths(&["photos"]),
            &Push {
                all: true,
                ..Push::default()
            },
        )
        .unwrap();
        assert!(
            reports
                .iter()
                .all(|r| r.unchanged == 0 && r.same.len() == 2)
        );
        assert!(s.engine.has("aws", "photos/2019/a.jpg"));

        // A changed file is checked again (and differs); the rest aren't.
        std::fs::write(s.root.join("photos/2019/b.jpg"), "bbbbb").unwrap();
        let reports = s.push(&["photos"]);
        assert!(
            reports
                .iter()
                .all(|r| r.unchanged == 1 && r.differs == ["photos/2019/b.jpg"])
        );
    }

    #[test]
    fn a_change_pushed_to_one_cloud_unchecks_the_others() {
        let mut s = setup("uncheck");
        s.push(&["notes.txt"]);
        std::fs::write(s.root.join("notes.txt"), "new").unwrap();
        let aws = vec![s.clouds[0].clone()];
        push(
            &s.engine,
            &s.root,
            &mut s.manifest,
            &aws,
            &paths(&["notes.txt"]),
            &Push {
                replace: true,
                ..Push::default()
            },
        )
        .unwrap();
        let entry = &s.manifest.files["notes.txt"];
        assert!(entry.clouds["aws"].checked.is_some());
        assert!(entry.clouds["drive"].checked.is_none());
        // So the next push to every cloud sends it to drive.
        let reports = s.push(&["notes.txt"]);
        assert_eq!(reports[0].unchanged, 1);
        assert_eq!(reports[1].differs, ["notes.txt"]);
    }

    #[test]
    fn push_never_replaces_a_differing_copy_unless_asked() {
        let mut s = setup("replace");
        s.push(&["notes.txt"]);
        std::fs::write(s.root.join("notes.txt"), "encrypted by ransomware").unwrap();
        let reports = s.push(&["notes.txt"]);
        assert!(
            reports
                .iter()
                .all(|r| r.differs == ["notes.txt"] && r.failed.is_some())
        );
        assert_eq!(s.engine.clouds.borrow()["aws"]["notes.txt"].0, b"n");
        let reports = push(
            &s.engine,
            &s.root,
            &mut s.manifest,
            &s.clouds,
            &paths(&["notes.txt"]),
            &Push {
                replace: true,
                ..Push::default()
            },
        )
        .unwrap();
        assert!(reports.iter().all(Checked::all_same));
    }

    #[test]
    fn clean_deletes_only_when_every_cloud_has_it() {
        let mut s = setup("clean");
        s.push(&["photos"]);
        let plan = s.clean(&["photos"]);
        assert!(plan.safe());
        assert_eq!(plan.bytes(), 7);
        let kept = apply_clean(&s.root, &plan).unwrap();
        assert!(kept.is_empty());
        assert!(!s.root.join("photos").exists());
        assert!(s.root.join("notes.txt").exists());
        let rows = tree(&s.root, &s.manifest, &s.clouds, "photos").unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.here == Here::Offloaded));
    }

    #[test]
    fn clean_refuses_a_missing_copy() {
        let mut s = setup("missing");
        s.push(&["photos"]);
        s.engine.remove("drive", "photos/2019/b.jpg");
        let plan = s.clean(&["photos"]);
        assert!(!plan.safe());
        assert_eq!(plan.checks[1].missing, ["photos/2019/b.jpg"]);
        assert!(apply_clean(&s.root, &plan).is_err());
        assert!(s.root.join("photos/2019/a.jpg").exists());
        assert!(s.root.join("photos/2019/b.jpg").exists());
    }

    #[test]
    fn clean_refuses_a_corrupt_copy() {
        let mut s = setup("corrupt");
        s.push(&["photos"]);
        s.engine.put("aws", "photos/2019/a.jpg", b"bitrot");
        let plan = s.clean(&["photos"]);
        assert!(!plan.safe());
        assert_eq!(plan.checks[0].differs, ["photos/2019/a.jpg"]);
        assert!(s.manifest.files["photos/2019/a.jpg"].clouds["aws"].differs);
    }

    #[test]
    fn clean_checks_a_newly_connected_cloud_too() {
        let mut s = setup("newcloud");
        s.push(&["photos"]);
        s.clouds.push(cloud("gcp", Kind::Gcp));
        let plan = s.clean(&["photos"]);
        assert!(!plan.safe());
        assert_eq!(plan.checks[2].missing.len(), 2);
    }

    #[test]
    fn clean_refuses_without_clouds() {
        let mut s = setup("noclouds");
        s.clouds.clear();
        assert!(
            plan_clean(
                &s.engine,
                &s.root,
                &mut s.manifest,
                &s.clouds,
                &paths(&["photos"])
            )
            .is_err()
        );
    }

    #[test]
    fn clean_keeps_a_file_changed_after_the_check() {
        let mut s = setup("changed");
        s.push(&["notes.txt"]);
        let plan = s.clean(&["notes.txt"]);
        std::fs::write(s.root.join("notes.txt"), "new words").unwrap();
        let kept = apply_clean(&s.root, &plan).unwrap();
        assert_eq!(kept, ["notes.txt"]);
        assert!(s.root.join("notes.txt").exists());
    }

    #[test]
    fn pull_reads_from_the_cheapest_cloud_and_restores_archives() {
        let mut s = setup("pull");
        s.push(&["photos"]);
        s.offload(&["photos"]);
        let plan = plan_pull(
            &s.root,
            &s.manifest,
            &s.clouds,
            &paths(&["photos/**/*.jpg"]),
        )
        .unwrap();
        assert_eq!(plan.download.len(), 1);
        assert_eq!(plan.download[0].0.name, "drive");
        let reports = apply_pull(
            &s.engine,
            &s.root,
            &mut s.manifest,
            &plan,
            &Download::default(),
        )
        .unwrap();
        assert!(reports[0].all_same());
        assert_eq!(
            std::fs::read(s.root.join("photos/2019/a.jpg")).unwrap(),
            b"aaa"
        );
        // Everything is here again: nothing to pull.
        let plan = plan_pull(&s.root, &s.manifest, &s.clouds, &paths(&["photos"])).unwrap();
        assert!(plan.download.is_empty());
        assert_eq!(plan.here.len(), 2);

        // Only an archived copy: restore it first.
        s.clouds.remove(1);
        s.offload(&["photos"]);
        s.manifest
            .files
            .get_mut("photos/2019/a.jpg")
            .unwrap()
            .clouds
            .get_mut("aws")
            .unwrap()
            .class = Some("DEEP_ARCHIVE".into());
        let plan = plan_pull(&s.root, &s.manifest, &s.clouds, &paths(&["photos"])).unwrap();
        assert_eq!(plan.restore[0].1, ["photos/2019/a.jpg"]);
        assert_eq!(plan.download[0].1, ["photos/2019/b.jpg"]);
        s.engine
            .archived
            .borrow_mut()
            .push("photos/2019/a.jpg".into());
        let reports = apply_pull(
            &s.engine,
            &s.root,
            &mut s.manifest,
            &plan,
            &Download::default(),
        )
        .unwrap();
        assert_eq!(*s.engine.restored.borrow(), ["photos/2019/a.jpg"]);
        assert_eq!(reports[0].waiting, ["photos/2019/a.jpg"]);
        assert!(reports.iter().all(Checked::all_same));
        assert!(!s.root.join("photos/2019/a.jpg").exists());
        assert!(s.root.join("photos/2019/b.jpg").exists());
        // Once the restore is done, the same pull gets it.
        s.engine.archived.borrow_mut().clear();
        let plan = plan_pull(&s.root, &s.manifest, &s.clouds, &paths(&["photos"])).unwrap();
        let reports = apply_pull(
            &s.engine,
            &s.root,
            &mut s.manifest,
            &plan,
            &Download::default(),
        )
        .unwrap();
        assert_eq!(reports[0].same, ["photos/2019/a.jpg"]);
        assert!(reports[0].waiting.is_empty());
        assert_eq!(
            std::fs::read(s.root.join("photos/2019/a.jpg")).unwrap(),
            b"aaa"
        );
    }

    #[test]
    fn pull_never_overwrites_a_changed_file() {
        let mut s = setup("overwrite");
        s.push(&["notes.txt"]);
        std::fs::write(s.root.join("notes.txt"), "mine").unwrap();
        let plan = plan_pull(&s.root, &s.manifest, &s.clouds, &paths(&["notes.txt"])).unwrap();
        let reports = apply_pull(
            &s.engine,
            &s.root,
            &mut s.manifest,
            &plan,
            &Download::default(),
        )
        .unwrap();
        assert!(reports[0].failed.is_some());
        assert_eq!(std::fs::read(s.root.join("notes.txt")).unwrap(), b"mine");
    }

    #[test]
    fn remove_refuses_the_last_copy() {
        let mut s = setup("remove");
        s.push(&["photos"]);
        s.offload(&["photos"]);
        let from = vec![s.clouds[0].clone()];
        let plan = plan_remove(
            &s.engine,
            &s.root,
            &s.manifest,
            &s.clouds,
            &from,
            &paths(&["photos"]),
        )
        .unwrap();
        assert!(plan.last_copies.is_empty());
        apply_remove(&s.engine, &mut s.manifest, &plan).unwrap();
        assert!(!s.engine.has("aws", "photos/2019/a.jpg"));
        let plan = plan_remove(
            &s.engine,
            &s.root,
            &s.manifest,
            &s.clouds,
            &s.clouds,
            &paths(&["photos"]),
        )
        .unwrap();
        assert_eq!(plan.last_copies.len(), 2);
        assert_eq!(only_in(Some(&s.root), &s.manifest, "drive").len(), 2);
        // Where the folder is unknown, a file here counts as well.
        std::fs::create_dir_all(s.root.join("photos/2019")).unwrap();
        std::fs::write(s.root.join("photos/2019/a.jpg"), b"back").unwrap();
        assert_eq!(only_in(Some(&s.root), &s.manifest, "drive").len(), 1);
        assert_eq!(only_in(None, &s.manifest, "drive").len(), 2);
    }

    #[test]
    fn refresh_rebuilds_from_listings() {
        let mut s = setup("refresh");
        s.push(&["notes.txt"]);
        s.engine
            .put("aws", "elsewhere.txt", b"from another machine");
        s.engine.remove("aws", "notes.txt");
        refresh(&s.engine, &mut s.manifest, &s.clouds[0]).unwrap();
        assert!(!s.manifest.files["notes.txt"].clouds.contains_key("aws"));
        let copy = &s.manifest.files["elsewhere.txt"].clouds["aws"];
        assert_eq!(copy.checked, None);
        let rows = tree(&s.root, &s.manifest, &s.clouds, "").unwrap();
        let other = rows.iter().find(|r| r.path == "elsewhere.txt").unwrap();
        assert_eq!(other.here, Here::Offloaded);
        assert_eq!(other.clouds[0].1, Mark::Unchecked);
    }

    #[test]
    fn a_listed_copy_of_a_file_here_is_not_a_change() {
        let mut s = setup("listed");
        s.engine.put("aws", "notes.txt", b"n");
        s.engine.put("aws", "photos/2019/a.jpg", b"other");
        refresh(&s.engine, &mut s.manifest, &s.clouds[0]).unwrap();
        let rows = tree(&s.root, &s.manifest, &s.clouds, "").unwrap();
        let notes = rows.iter().find(|r| r.path == "notes.txt").unwrap();
        assert_eq!(notes.here, Here::Here);
        assert_eq!(notes.clouds[0].1, Mark::Unchecked);
        let a = rows.iter().find(|r| r.path == "photos/2019/a.jpg").unwrap();
        assert_eq!(a.here, Here::Changed);
    }
}
