use super::*;
use link_test_kit::TempDir;

fn manifest(phase: Phase, removed: &[&str]) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "format": PROTOCOL.current, "phase": phase, "move": "0123456789abcdef0123456789abcdef",
        "removed": removed, "agent": {"workload": "agent:a1b2c3d4e5f6", "name": "main"},
    }))
    .unwrap()
}

/// A manifest states the move protocol's version: an older one, a
/// newer one or none is refused by name.
#[test]
fn a_manifest_of_another_move_protocol_is_refused_by_name() {
    let of = |format: serde_json::Value| check_format(&serde_json::json!({"format": format}));
    assert!(of(PROTOCOL.current.into()).is_ok());
    let older = of((PROTOCOL.oldest - 1).into()).unwrap_err().to_string();
    assert!(
        older.contains("below") && older.ends_with("lnk upgrade"),
        "{older}"
    );
    let newer = of((PROTOCOL.current + 1).into()).unwrap_err().to_string();
    assert!(newer.contains("newer than"), "{newer}");
    assert!(of(serde_json::Value::Null).is_err());
}

#[test]
fn a_folder_round_trips_and_what_is_held_is_skipped() {
    let dir = TempDir::new("moving-round");
    let (from, to, spool) = (dir.join("from"), dir.join("to"), dir.join("spool"));
    std::fs::create_dir_all(from.join("notes")).unwrap();
    std::fs::write(from.join("notes/a.md"), "a").unwrap();
    std::fs::write(from.join("b.txt"), "b".repeat(100)).unwrap();
    std::os::unix::fs::symlink("notes/a.md", from.join("latest")).unwrap();
    let big = vec![7u8; (IN_MEMORY + 10) as usize];
    std::fs::write(from.join("big"), &big).unwrap();
    let stream = |have: &Have| {
        let mut tar = tar::Builder::new(Vec::new());
        append_manifest(&mut tar, &manifest(Phase::Copy, &[])).unwrap();
        let files = append_folder(&mut tar, &from, "files", have, None, &spool).unwrap();
        append_end(&mut tar).unwrap();
        (tar.into_inner().unwrap(), files)
    };
    let (bytes, files) = stream(&Have::new());
    assert_eq!(files.len(), 3);
    let unpacked = unpack(bytes.as_slice(), |_, _| Ok(to.clone())).unwrap();
    assert_eq!(unpacked.head.phase, Phase::Copy);
    assert_eq!(std::fs::read(to.join("files/big")).unwrap(), big);
    assert_eq!(
        std::fs::read_link(to.join("files/latest")).unwrap(),
        Path::new("notes/a.md")
    );
    // The spool leaves nothing behind.
    assert_eq!(std::fs::read_dir(&spool).unwrap().count(), 0);
    // What the other end holds isn't sent again.
    let have = have_of(&to.join("files"), "files").unwrap();
    assert_eq!(have, have_of(&from, "files").unwrap());
    let (again, _) = stream(&have);
    assert!(again.len() < 8 * 1024, "{}", again.len());
}

/// An entry whose bytes were changed on the way, or that has no
/// hash, refuses the stream; a format-2 stream is refused by name.
#[test]
fn a_tampered_entry_is_refused() {
    let dir = TempDir::new("moving-tampered");
    let (from, to, spool) = (dir.join("from"), dir.join("to"), dir.join("spool"));
    std::fs::create_dir_all(&from).unwrap();
    std::fs::write(from.join("note"), "the original text").unwrap();
    let mut tar = tar::Builder::new(Vec::new());
    append_manifest(&mut tar, &manifest(Phase::Copy, &[])).unwrap();
    append_folder(&mut tar, &from, "files", &Have::new(), None, &spool).unwrap();
    append_end(&mut tar).unwrap();
    let mut bytes = tar.into_inner().unwrap();
    // Cut short, between two entries: refused.
    let cut = bytes.len() - 1024 - 3 * 512;
    let err = unpack(&bytes[..cut], |_, _| Ok(to.clone())).unwrap_err();
    assert!(
        format!("{err:#}").contains("stopped before its end"),
        "{err:#}"
    );
    let at = bytes.windows(8).position(|w| w == b"original").unwrap();
    bytes[at..at + 8].copy_from_slice(b"ORIGINAL");
    let err = unpack(bytes.as_slice(), |_, _| Ok(to.clone())).unwrap_err();
    assert!(format!("{err:#}").contains("SHA-256 differs"), "{err:#}");
    assert!(!to.join("files/note").exists());

    // No hash at all.
    let mut tar = tar::Builder::new(Vec::new());
    append_manifest(&mut tar, &manifest(Phase::Copy, &[])).unwrap();
    let mut header = tar::Header::new_gnu();
    header.set_size(3);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "files/bare", &b"abc"[..])
        .unwrap();
    let err = unpack(tar.into_inner().unwrap().as_slice(), |_, _| Ok(to.clone())).unwrap_err();
    assert!(
        format!("{err:#}").contains("without its SHA-256"),
        "{err:#}"
    );

    // A folder the phase doesn't allow.
    let mut tar = tar::Builder::new(Vec::new());
    append_manifest(&mut tar, &manifest(Phase::Copy, &[])).unwrap();
    append_folder(&mut tar, &from, "connections", &Have::new(), None, &spool).unwrap();
    let err = unpack(tar.into_inner().unwrap().as_slice(), |_, _| Ok(to.clone())).unwrap_err();
    assert!(format!("{err:#}").contains("isn't in files/"), "{err:#}");
}

#[test]
fn paths_and_haves_are_checked() {
    for good in ["files/a", "memory/link/index.sqlite", "files/.hidden/x"] {
        check_path(good, &["files", "memory"]).unwrap();
    }
    for bad in [
        "",
        "/etc/passwd",
        "files/../x",
        "files/./x",
        "files//x",
        "other/x",
        "files/\0",
    ] {
        assert!(check_path(bad, &["files", "memory"]).is_err(), "{bad:?}");
    }
    check_move_id("0123456789abcdef0123456789abcdef").unwrap();
    assert!(check_move_id("0123456789ABCDEF0123456789abcdef").is_err());
    assert!(check_move_id("\u{1b}[2J").is_err());
    let have = read_have(
        &br#"{"files/a": "0000000000000000000000000000000000000000000000000000000000000000"}"#[..],
    )
    .unwrap();
    assert_eq!(have.len(), 1);
    assert!(read_have(&br#"{"../a": "00"}"#[..]).is_err());
    assert_eq!(new_move_id().unwrap().len(), 32);
}

/// A path deeper than [`DEEPEST`] folders below its phase's folder is
/// refused, as the sender never adds one.
#[test]
fn a_stream_is_32_folders_deep_at_most() {
    let at = |depth: usize| {
        let mut path = vec!["files"];
        path.extend(std::iter::repeat_n("d", depth));
        path.join("/")
    };
    check_path(&at(DEEPEST), &["files"]).unwrap();
    let refused = check_path(&at(DEEPEST + 1), &["files"]).unwrap_err();
    assert!(
        format!("{refused:#}").contains("folders deep"),
        "{refused:#}"
    );
}

/// An extension entry over its cap is refused before it's read whole,
/// and an ordinary PAX hash, and a PAX size, still pass.
#[test]
fn a_streams_extension_entries_are_bounded() {
    let dir = TempDir::new("moving-extension");
    let (from, to, spool) = (dir.join("from"), dir.join("to"), dir.join("spool"));
    std::fs::create_dir_all(&from).unwrap();
    std::fs::write(from.join("note"), "kept").unwrap();
    // An ordinary stream, its hashes in PAX headers: taken.
    let mut tar = tar::Builder::new(Vec::new());
    append_manifest(&mut tar, &manifest(Phase::Copy, &[])).unwrap();
    append_folder(&mut tar, &from, "files", &Have::new(), None, &spool).unwrap();
    append_end(&mut tar).unwrap();
    unpack(tar.into_inner().unwrap().as_slice(), |_, _| Ok(to.clone())).unwrap();
    assert_eq!(std::fs::read(to.join("files/note")).unwrap(), b"kept");
    // A PAX header past the cap: refused, naming it.
    let mut tar = tar::Builder::new(Vec::new());
    append_manifest(&mut tar, &manifest(Phase::Copy, &[])).unwrap();
    let big = "x".repeat(MAX_EXTENSION as usize);
    tar.append_pax_extensions([("LNK.padding", big.as_bytes())])
        .unwrap();
    let err = unpack(tar.into_inner().unwrap().as_slice(), |_, _| Ok(to.clone())).unwrap_err();
    assert!(format!("{err:#}").contains("extension entry"), "{err:#}");
    // A PAX size is followed to the next header: what follows is read
    // as its own entry, not as data.
    let bytes = {
        let mut tar = tar::Builder::new(Vec::new());
        append_manifest(&mut tar, &manifest(Phase::Copy, &[])).unwrap();
        let data = b"sized by pax";
        let hash = sha256_of(&data[..]).unwrap();
        let size = data.len().to_string();
        tar.append_pax_extensions([(SHA256, hash.as_bytes()), ("size", size.as_bytes())])
            .unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "files/sized", &data[..])
            .unwrap();
        append_end(&mut tar).unwrap();
        tar.into_inner().unwrap()
    };
    let to = dir.join("sized");
    unpack(bytes.as_slice(), |_, _| Ok(to.clone())).unwrap();
    assert_eq!(
        std::fs::read(to.join("files/sized")).unwrap(),
        b"sized by pax"
    );
}

/// Tar's size fields, octal and base-256, and a PAX size record.
#[test]
fn header_sizes_are_read_both_ways() {
    assert_eq!(header_size(b"00000001750\0"), Some(1000));
    let mut binary = [0u8; 12];
    binary[0] = 0x80;
    binary[11] = 0x10;
    assert_eq!(header_size(&binary), Some(16));
    assert_eq!(pax_size(b"11 size=42\n"), Some(42));
    assert_eq!(pax_size(b"9 a=bcd\n"), None);
}

#[test]
fn a_link_on_the_way_is_never_opened_through() {
    let dir = TempDir::new("moving-open");
    std::fs::create_dir_all(dir.join("outside")).unwrap();
    std::fs::write(dir.join("outside/secret"), "secret").unwrap();
    std::fs::create_dir_all(dir.join("root")).unwrap();
    std::os::unix::fs::symlink(dir.join("outside"), dir.join("root/out")).unwrap();
    std::os::unix::fs::symlink(dir.join("outside/secret"), dir.join("root/s")).unwrap();
    assert!(open_under(&dir.join("root"), Path::new("out/secret")).is_err());
    assert!(open_under(&dir.join("root"), Path::new("s")).is_err());
    assert!(have_of(&dir.join("root"), "files").unwrap().is_empty());
    remove_under(&dir.join("root"), "files/x").unwrap();
}
