use super::*;
use link_store_contract::store::{Store as _, Written};

struct Here(store::objects::Objects<Bucket>);
impl store::Store for Here {
    fn name(&self) -> &str {
        "bucket"
    }
    fn call(&self, request: &Request) -> anyhow::Result<Reply> {
        self.0.answer(request)
    }
}
fn agent() -> Agent {
    Agent {
        id: "aaaaaaaaaaaa".into(),
        owner: "me@box".into(),
    }
}
fn here() -> Option<(link_test_kit::TempDir, Here)> {
    if !link_test_kit::gate::program("python3", &["--version"]) {
        return None;
    }
    let dir = link_test_kit::TempDir::new("bucket-objects");
    let program = dir.join("rclone");
    link_test_kit::write_program(&program, include_str!("tests/engine.py")).unwrap();
    for folder in ["config", "objects", "segments"] {
        std::fs::create_dir(dir.join(folder)).unwrap();
    }
    let bucket = Bucket {
        rclone: std::sync::Mutex::new(Rclone::with(program, &dir.join("config")).unwrap()),
        root: format!("{}/", dir.join("objects").display()),
        lock: dir.join("lock"),
    };
    Some((dir, Here(store::objects::Objects::new(bucket, agent()))))
}

#[test]
fn bucket_files_obey_the_shared_contract_with_content_hash_cas() {
    let Some((_dir, here)) = here() else { return };
    store::check_files(&here, &agent());
}

#[test]
fn equal_bucket_payloads_never_revive_old_file_revisions() {
    let Some((_dir, here)) = here() else { return };
    let agent = agent();
    let path = "instructions/instruction.md";
    let mut seen = std::collections::BTreeSet::new();
    let mut expected = 0;
    for bytes in [b"A", b"B", b"A"] {
        let Written::Version(version) = here.file_write(&agent, path, bytes, expected).unwrap()
        else {
            panic!("not written")
        };
        assert!(seen.insert(version));
        expected = version;
    }
    assert!(here.file_remove(&agent, path, expected).unwrap());
    assert_eq!(here.file_read(&agent, path).unwrap(), None);
    assert_eq!(
        here.file_list(&agent, "instructions/").unwrap(),
        Vec::<String>::new()
    );
    let Written::Version(again) = here.file_write(&agent, path, b"A", 0).unwrap() else {
        panic!("not recreated")
    };
    assert!(seen.insert(again));
    for old in seen.into_iter().filter(|version| *version != again) {
        assert_eq!(
            here.file_write(&agent, path, b"stale", old).unwrap(),
            Written::Stale(again)
        );
        assert!(!here.file_remove(&agent, path, old).unwrap());
    }
}

#[test]
fn maximum_memory_payload_roundtrips_through_the_bucket_envelope() {
    let Some((_dir, here)) = here() else { return };
    let agent = agent();
    let path = "memory/notes.md";
    let body = vec![b'x'; store::MEMORY_FILE_MAX as usize];
    let Written::Version(version) = here.file_write(&agent, path, &body, 0).unwrap() else {
        panic!("not written")
    };
    assert_eq!(
        here.file_read(&agent, path).unwrap(),
        Some((body.clone(), version))
    );
    assert!(here.file_append(&agent, path, b"x").is_err());
    assert_eq!(here.file_read(&agent, path).unwrap(), Some((body, version)));
}

#[test]
fn content_named_files_read_current_revisions_and_tombstones_from_the_backend() {
    let Some((dir, here)) = here() else { return };
    let agent = agent();
    let payload = b"immutable segment";
    let hash = link_digest::sha256(payload);
    let path = format!("memory/engine/seg-{hash}");
    let cached = dir.join("segments").join(hash);
    let Written::Version(first) = here.file_write(&agent, &path, payload, 0).unwrap() else {
        panic!("not written")
    };
    let object = dir.join("objects").join(&path);
    // A valid cached old record authenticates the bytes but cannot prove
    // that its revision is still the backend's current one.
    std::fs::write(&cached, std::fs::read(&object).unwrap()).unwrap();
    let mut current: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&object).unwrap()).unwrap();
    current["revision"] = (first + 1).into();
    std::fs::write(&object, serde_json::to_vec(&current).unwrap()).unwrap();
    assert_eq!(
        here.file_read(&agent, &path).unwrap(),
        Some((payload.to_vec(), first + 1))
    );
    assert_eq!(
        here.file_write(&agent, &path, b"stale", first).unwrap(),
        Written::Stale(first + 1)
    );
    current["revision"] = (first + 2).into();
    current["data"] = serde_json::Value::Null;
    std::fs::write(&object, serde_json::to_vec(&current).unwrap()).unwrap();
    assert_eq!(here.file_read(&agent, &path).unwrap(), None);
    assert_eq!(
        here.file_list(&agent, "memory/engine/").unwrap(),
        Vec::<String>::new()
    );
}
