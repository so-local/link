use super::*;
use crate::files::DirStore;

fn here(name: &str) -> (link_test_kit::TempDir, Objects<DirStore>) {
    let root = link_test_kit::TempDir::new(name);
    let store = DirStore::new(&root.join("objects")).unwrap();
    let agent = Agent {
        id: "aaaaaaaaaaaa".into(),
        owner: "me@box".into(),
    };
    (root, Objects::new(store, agent))
}

#[test]
fn tombstones_are_absent_to_readers_and_lists_but_keep_revision_history() {
    let (_root, objects) = here("object-tombstone");
    let path = "instructions/instruction.md";
    let Reply::Wrote { version: first } = objects.write(path, b"A", 0).unwrap() else {
        panic!("not written")
    };
    assert!(objects.remove_file(path, first).unwrap());
    assert_eq!(objects.file(path).unwrap(), None);
    assert!(
        objects.store.get(path).unwrap().is_some(),
        "history remains after removal"
    );
    let reply = objects
        .answer(&Request::FileList {
            agent: objects.agent.clone(),
            folder: "instructions/".into(),
        })
        .unwrap();
    assert_eq!(reply, Reply::Files { files: Vec::new() });
    let Reply::Wrote { version: again } = objects.write(path, b"A", 0).unwrap() else {
        panic!("not recreated")
    };
    assert!(again > first);
    assert_eq!(
        objects.write(path, b"stale", first).unwrap(),
        Reply::Stale { version: again }
    );
    assert!(!objects.remove_file(path, first).unwrap());
    assert_eq!(objects.file(path).unwrap(), Some((b"A".to_vec(), again)));
}

#[test]
fn malformed_or_exhausted_file_records_refuse_without_replacing_them() {
    let (_root, objects) = here("object-file-invalid");
    let path = "instructions/instruction.md";
    for bytes in [
        b"raw text".as_slice(),
        br#"{}"#,
        br#"{"revision":0,"data":null}"#,
        br#"{"revision":1,"data":"%%%"}"#,
        br#"{"revision":1,"data":null,"surprise":true}"#,
    ] {
        objects.store.put(path, bytes, &Expect::Any).unwrap();
        assert!(objects.file(path).is_err());
        assert!(objects.write(path, b"B", 0).is_err());
        assert!(objects.append_file(path, b"B").is_err());
        assert!(objects.remove_file(path, 1).is_err());
        assert_eq!(objects.store.get(path).unwrap().unwrap().0, bytes);
    }
    for data in [None, Some("QQ==".into())] {
        let record = FileRecord {
            revision: u64::MAX,
            data,
        };
        let encoded = link_files::saved::print_json(&record).unwrap();
        objects.store.put(path, &encoded, &Expect::Any).unwrap();
        assert!(objects.write(path, b"B", record.visible_version()).is_err());
        assert!(objects.append_file(path, b"B").is_err());
        if record.data.is_some() {
            assert!(objects.remove_file(path, u64::MAX).is_err());
        }
        assert_eq!(objects.store.get(path).unwrap().unwrap().0, encoded);
    }
}

#[test]
fn logical_payload_caps_include_the_encoded_metadata_envelope() {
    let (_root, objects) = here("object-file-bounds");
    for path in ["instructions/instruction.md", "memory/notes.md"] {
        let cap = file_max(path);
        let record = FileRecord {
            revision: u64::MAX,
            data: Some(base64::engine::general_purpose::STANDARD.encode(vec![b'x'; cap as usize])),
        };
        let encoded = link_files::saved::print_json(&record).unwrap();
        assert!(encoded.len() as u64 <= file_record_max(cap).unwrap());
        assert!(encoded.len() as u64 > cap);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&encoded).unwrap()["schema"],
            1
        );
        let mut future: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        future["schema"] = u32::MAX.into();
        let future = serde_json::to_vec(&future).unwrap();
        assert!(future.len() as u64 <= file_record_max(cap).unwrap());
        objects.store.put(path, &future, &Expect::Any).unwrap();
        let error = objects.file(path).unwrap_err();
        assert!(format!("{error:#}").contains("lnk upgrade"), "{error:#}");
        assert_eq!(objects.store.get(path).unwrap().unwrap().0, future);
        objects.store.put(path, &encoded, &Expect::Any).unwrap();
        assert_eq!(objects.file(path).unwrap().unwrap().0.len() as u64, cap);
        let too_large = FileRecord {
            revision: 1,
            data: Some(
                base64::engine::general_purpose::STANDARD.encode(vec![b'x'; cap as usize + 1]),
            ),
        };
        let encoded = link_files::saved::print_json(&too_large).unwrap();
        objects.store.put(path, &encoded, &Expect::Any).unwrap();
        assert!(objects.file(path).is_err());
        assert!(objects.append_file(path, b"x").is_err());
        assert_eq!(objects.store.get(path).unwrap().unwrap().0, encoded);
    }
}

#[test]
fn future_file_records_require_an_upgrade_before_any_mutation() {
    let (_root, objects) = here("object-file-schema");
    let path = "instructions/instruction.md";
    let bytes = br#"{"schema":2,"revision":1,"data":"QQ=="}"#;
    objects.store.put(path, bytes, &Expect::Absent).unwrap();
    for error in [
        objects.file(path).unwrap_err(),
        objects.write(path, b"B", 1).unwrap_err(),
        objects.append_file(path, b"B").unwrap_err(),
        objects.remove_file(path, 1).unwrap_err(),
    ] {
        assert!(format!("{error:#}").contains("lnk upgrade"), "{error:#}");
    }
    assert_eq!(objects.store.get(path).unwrap().unwrap().0, bytes);
}
