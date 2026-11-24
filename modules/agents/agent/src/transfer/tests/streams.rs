//! A stream refused where it would reach out of its folder, and each
//! linked connection carried without its install.

use super::*;

/// A tar entry written as a hostile sender would: any name, any
/// link, which `tar::Builder` refuses to write.
fn entry(tar: &mut Vec<u8>, name: &str, kind: tar::EntryType, link: &str, data: &[u8]) {
    let mut header = tar::Header::new_old();
    header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
    header.as_old_mut().linkname[..link.len()].copy_from_slice(link.as_bytes());
    header.set_entry_type(kind);
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.extend_from_slice(header.as_bytes());
    tar.extend_from_slice(data);
    tar.resize(tar.len().next_multiple_of(512), 0);
}

#[cfg(unix)]
#[test]
fn a_stream_sets_no_setuid_bit() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp("setuid");
    let mut tar = Vec::new();
    for (name, kind, mode) in [
        ("files/tool", tar::EntryType::Regular, 0o4755),
        ("files/group", tar::EntryType::Regular, 0o2750),
        ("files/shared", tar::EntryType::Directory, 0o1777),
        ("files/mine", tar::EntryType::Regular, 0o600),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_path(name).unwrap();
        header.set_entry_type(kind);
        header.set_size(0);
        header.set_mode(mode);
        header.set_cksum();
        tar.extend_from_slice(header.as_bytes());
    }
    tar.extend_from_slice(&[0; 1024]);
    unpack_in(tar.as_slice(), &dir, Links::Keep).unwrap();
    let mode = |name: &str| {
        std::fs::symlink_metadata(dir.join("files").join(name))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    };
    assert_eq!(mode("tool"), 0o755);
    assert_eq!(mode("group"), 0o750);
    assert_eq!(mode("shared"), 0o777);
    assert_eq!(mode("mine"), 0o600);
}

#[cfg(unix)]
#[test]
fn a_hostile_stream_stays_in_its_folder() {
    use tar::EntryType::{Link, Regular, Symlink};
    // Short, as these paths go in a tar header of 100 bytes.
    let dir = link_test_kit::TempDir::short("hostile");
    let outside = dir.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret"), "secret").unwrap();
    let out = outside.to_str().unwrap().to_string();
    let secret = format!("{out}/secret");
    let cases: [&[(&str, tar::EntryType, &str)]; 7] = [
        &[("../outside/up", Regular, "")],
        &[(&format!("{out}/absolute"), Regular, "")],
        // A link out, then a file through it.
        &[
            ("harness/out", Symlink, &out),
            ("harness/out/through", Regular, ""),
        ],
        &[
            ("harness/up", Symlink, "../.."),
            ("harness/up/outside/through", Regular, ""),
        ],
        // A hard link to a file outside, then written through.
        &[
            ("harness/hard", Link, &secret),
            ("harness/hard", Regular, ""),
        ],
        &[
            ("harness/hard", Link, "../outside/secret"),
            ("harness/hard", Regular, ""),
        ],
        // A link to a file outside, then a file in its place.
        &[
            ("files/over", Symlink, &secret),
            ("files/over", Regular, ""),
        ],
    ];
    for (i, case) in cases.iter().enumerate() {
        let staging = dir.join(format!("staging-{i}"));
        std::fs::create_dir_all(&staging).unwrap();
        let mut tar = Vec::new();
        for (name, kind, link) in *case {
            let data: &[u8] = if *kind == Regular {
                b"overwritten"
            } else {
                b""
            };
            entry(&mut tar, name, *kind, link, data);
        }
        tar.resize(tar.len() + 1024, 0);
        let _ = unpack_in(tar.as_slice(), &staging, Links::Keep);
        let names: Vec<_> = std::fs::read_dir(&outside)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["secret"], "case {i}: {case:?}");
        assert_eq!(
            std::fs::read_to_string(outside.join("secret")).unwrap(),
            "secret",
            "case {i}: {case:?}"
        );
    }
    // A link that came is carried as a link: the files folder gets
    // it, and what it points to is left alone.
    let files = dir.join("files");
    std::fs::create_dir_all(&files).unwrap();
    merge(&dir.join("staging-2/harness"), &files).unwrap();
    assert!(files.join("out").symlink_metadata().unwrap().is_symlink());
    assert_eq!(
        std::fs::read_to_string(outside.join("secret")).unwrap(),
        "secret"
    );
}

/// Signal's state, as `lnk-signal state` says.
pub(super) fn signal() -> State {
    State {
        include: vec!["~/.signal-cli/data".into(), "~/.signal-cli/account".into()],
        exclude: Vec::new(),
    }
}

#[test]
fn a_linked_connection_moves_without_its_install() {
    let dir = temp("connection");
    let from = dir.join("from/signal");
    let cli = from.join(".signal-cli");
    std::fs::create_dir_all(cli.join("data/data")).unwrap();
    std::fs::create_dir_all(cli.join("0.14.8/bin")).unwrap();
    std::fs::create_dir_all(cli.join("java")).unwrap();
    std::fs::write(cli.join("data/data/accounts.json"), "linked").unwrap();
    std::fs::write(cli.join("account"), "+15557654321").unwrap();
    std::fs::write(cli.join("0.14.8/bin/signal-cli"), "a Mac program").unwrap();
    std::fs::write(cli.join("java/release"), "a Mac runtime").unwrap();
    std::fs::write(cli.join("signal-cli"), "the wrapper").unwrap();
    let mut out = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut out);
        add_state(
            &mut tar,
            &from,
            &signal(),
            Path::new("connections/signal"),
            &Out::none(),
        )
        .unwrap();
        tar.finish().unwrap();
    }
    let mut names: Vec<String> = tar::Archive::new(out.as_slice())
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().display().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "connections/signal/.signal-cli/account",
            "connections/signal/.signal-cli/data",
            "connections/signal/.signal-cli/data/data",
            "connections/signal/.signal-cli/data/data/accounts.json",
        ]
    );

    let staging = dir.join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    unpack_in(out.as_slice(), &staging, Links::Keep).unwrap();
    // This machine's own install stays, and an old account goes.
    let to = dir.join("to");
    let here = to.join("signal/.signal-cli");
    std::fs::create_dir_all(here.join("0.14.8/bin")).unwrap();
    std::fs::create_dir_all(here.join("data")).unwrap();
    std::fs::write(here.join("0.14.8/bin/signal-cli"), "a Linux program").unwrap();
    std::fs::write(here.join("data/stale"), "old").unwrap();
    let connections = BTreeMap::from([("signal".to_string(), signal())]);
    put_connections(
        &staging,
        connections.keys(),
        &|c| c == "signal",
        &|_| Some(signal()),
        |c| to.join(c),
    )
    .unwrap();
    let read = |p: &Path| std::fs::read_to_string(p).unwrap();
    assert_eq!(read(&here.join("data/data/accounts.json")), "linked");
    assert_eq!(read(&here.join("account")), "+15557654321");
    assert_eq!(read(&here.join("0.14.8/bin/signal-cli")), "a Linux program");
    assert!(!here.join("data/stale").exists());
    assert!(!here.join("java").exists() && !here.join("signal-cli").exists());
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &to.join("signal").metadata().unwrap().permissions(),
    );
    assert_eq!(mode & 0o777, 0o700);
}

#[cfg(unix)]
#[test]
fn a_hostile_connection_stays_in_its_folder() {
    use tar::EntryType::{Regular, Symlink};
    // Short, as these paths go in a tar header of 100 bytes.
    let dir = link_test_kit::TempDir::short("hostile-conn");
    let outside = dir.join("outside");
    std::fs::create_dir_all(outside.join("data")).unwrap();
    std::fs::write(outside.join("data/secret"), "secret").unwrap();
    let out = outside.to_str().unwrap().to_string();
    let staging = dir.join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    let mut tar = Vec::new();
    for (name, kind, link) in [
        ("connections/signal/.signal-cli/data/keys", Regular, ""),
        // A program the other side would run, outside any sandbox.
        (
            "connections/signal/.signal-cli/0.14.8/signal-cli",
            Regular,
            "",
        ),
        // A link, where the account's number goes.
        ("connections/signal/.signal-cli/account", Symlink, &*out),
        // A connection with a token, and one the manifest doesn't name.
        ("agents/connections/telegram/x", Regular, ""),
        ("agents/connections/matrix/matrix.db", Regular, ""),
        ("connections/../x", Regular, ""),
    ] {
        let data: &[u8] = if kind == Regular { b"came" } else { b"" };
        entry(&mut tar, name, kind, link, data);
    }
    tar.resize(tar.len() + 1024, 0);
    let _ = unpack_in(tar.as_slice(), &staging, Links::Keep);
    let to = dir.join("to");
    let connections = BTreeMap::from([
        ("signal".to_string(), signal()),
        ("telegram".to_string(), signal()),
        ("../x".to_string(), signal()),
    ]);
    put_connections(
        &staging,
        connections.keys(),
        &|c| c == "signal",
        &|_| Some(signal()),
        |c| to.join(c),
    )
    .unwrap();
    let cli = to.join("signal/.signal-cli");
    assert_eq!(
        std::fs::read_to_string(cli.join("data/keys")).unwrap(),
        "came"
    );
    assert!(!cli.join("0.14.8").exists());
    assert!(cli.join("account").symlink_metadata().is_err());
    assert!(!to.join("telegram").exists() && !to.join("matrix").exists());
    assert!(!dir.join("x").exists() && !to.join("x").exists());

    // A link in the place of a folder the state names: what it
    // points to stays where it is.
    let staging = dir.join("staging-link");
    std::fs::create_dir_all(&staging).unwrap();
    let mut tar = Vec::new();
    entry(
        &mut tar,
        "connections/signal/.signal-cli",
        Symlink,
        &out,
        b"",
    );
    tar.resize(tar.len() + 1024, 0);
    unpack_in(tar.as_slice(), &staging, Links::Keep).unwrap();
    let connections = BTreeMap::from([("signal".to_string(), signal())]);
    put_connections(
        &staging,
        connections.keys(),
        &|c| c == "signal",
        &|_| Some(signal()),
        |c| dir.join("to-link").join(c),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(outside.join("data/secret")).unwrap(),
        "secret"
    );
    assert!(!dir.join("to-link/signal/.signal-cli").exists());
}
