use super::*;
use std::io::{Read, Write};
use std::os::unix::fs::symlink;

fn temp(tag: &str) -> link_test_kit::TempDir {
    link_test_kit::TempDir::new(&format!("nofollow-{tag}"))
}

#[test]
fn search_parent_reaches_children_without_following_a_link() {
    let temp = temp("search-parent");
    let root = temp.canonicalize().unwrap();
    std::fs::create_dir(root.join("child")).unwrap();
    std::fs::write(root.join("child/event"), b"hello").unwrap();
    let parent = Dir::search_unlinked(&root).unwrap();
    let child = parent.folder(OsStr::new("child")).unwrap();
    assert_eq!(
        read_open(
            child
                .open_file(OsStr::new("event"), libc::O_RDONLY, 0)
                .unwrap(),
            5
        )
        .unwrap(),
        b"hello"
    );
    symlink(&root, root.join("alias")).unwrap();
    assert!(Dir::search_unlinked(&root.join("alias")).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn search_parent_in_seatbelt_keeps_other_children_private() {
    const ROOT: &str = "LNK_TEST_SEARCH_ROOT";
    if let Some(root) = std::env::var_os(ROOT).map(PathBuf::from) {
        assert_eq!(
            Dir::open_unlinked(&root).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        let parent = Dir::search_unlinked(&root).unwrap();
        let child = parent.folder(OsStr::new("child")).unwrap();
        assert_eq!(child.names().unwrap(), [OsString::from("event")]);
        assert!(
            parent
                .open_file(OsStr::new("secret"), libc::O_RDONLY, 0)
                .is_err()
        );
        return;
    }
    if !link_test_kit::gate::program(
        "/usr/bin/sandbox-exec",
        &["-p", "(version 1)(allow default)", "/usr/bin/true"],
    ) {
        return;
    }
    let temp = temp("search-seatbelt");
    let root = temp.canonicalize().unwrap();
    std::fs::create_dir(root.join("child")).unwrap();
    std::fs::write(root.join("child/event"), b"hello").unwrap();
    std::fs::write(root.join("secret"), b"private").unwrap();
    let path = root
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let profile = format!(
        "(version 1)(allow default)(deny file-read-data (literal \"{path}\") (literal \"{path}/secret\"))"
    );
    let output = link_test_kit::env::command("/usr/bin/sandbox-exec", &temp)
        .args(["-p", &profile])
        .arg(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "nofollow::tests::search_parent_in_seatbelt_keeps_other_children_private",
            "--nocapture",
        ])
        .env(ROOT, &root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

/// The one safe connect reaches a socket of this user's, and refuses a
/// link swapped in where the socket was, or a path that isn't a socket:
/// a sandboxed program can't redirect the connection.
#[test]
fn connect_reaches_only_a_socket_of_this_users() {
    use std::os::unix::net::{UnixListener, UnixStream};
    let dir = temp("connect");
    let root = dir.canonicalize().unwrap();
    let socket = root.join("in.sock");
    let _listener = UnixListener::bind(&socket).unwrap();
    // The real socket: reached.
    assert!(connect(&socket).is_ok());
    // A path that isn't a socket: refused, not connected.
    let file = root.join("file");
    std::fs::write(&file, b"x").unwrap();
    assert_eq!(
        connect(&file).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    // A link where the socket goes, pointing at a real socket elsewhere:
    // refused (O_NOFOLLOW), so a swapped link can't redirect the connect.
    let elsewhere = root.join("elsewhere.sock");
    let _other = UnixListener::bind(&elsewhere).unwrap();
    let link = root.join("link.sock");
    symlink(&elsewhere, &link).unwrap();
    assert!(connect(&link).is_err());
    // A plain connect would have followed the link and reached it.
    assert!(UnixStream::connect(&link).is_ok());
}

#[test]
fn a_link_is_never_followed() {
    let dir = temp("links");
    let outside = dir.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret"), "key").unwrap();
    let home = dir.join("home");
    std::fs::create_dir(&home).unwrap();
    symlink(&outside, home.join("folder")).unwrap();
    symlink(outside.join("secret"), home.join("file")).unwrap();

    let open = Dir::open(&home).unwrap();
    assert!(open.folder(OsStr::new("folder")).is_err());
    assert!(open.create_folder(OsStr::new("folder"), 0o700).is_err());
    let err = open.open_entry(OsStr::new("file")).unwrap_err();
    assert!(is_link(&err), "{err}");
    assert!(open.create_new(OsStr::new("file"), 0o600).is_err());
    assert_eq!(open.kind(OsStr::new("file")).unwrap(), Kind::Link);
    assert_eq!(
        open.read_link(OsStr::new("file")).unwrap(),
        outside.join("secret")
    );
    // Renaming over a link replaces the link, never its target.
    open.create_new(OsStr::new("new"), 0o600)
        .unwrap()
        .write_all(b"mine")
        .unwrap();
    open.rename(OsStr::new("new"), OsStr::new("file")).unwrap();
    assert_eq!(
        std::fs::read_to_string(outside.join("secret")).unwrap(),
        "key"
    );
    let mut read = String::new();
    open.open_entry(OsStr::new("file"))
        .unwrap()
        .read_to_string(&mut read)
        .unwrap();
    assert_eq!(read, "mine");
    assert_eq!(
        open.names().unwrap(),
        [OsString::from("file"), OsString::from("folder")]
    );
    // Truncating is refused before anything is opened.
    let err = open
        .open_file(OsStr::new("file"), libc::O_WRONLY | libc::O_TRUNC, 0)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
    assert_eq!(
        read_open(open.open_entry(OsStr::new("file")).unwrap(), 10).unwrap(),
        b"mine"
    );
}

/// A tree of folders alone, too wide or too deep, is refused past its
/// limit; one within it is copied.
#[test]
fn folders_too_many_or_too_deep_are_refused() {
    let dir = temp("limits");
    std::fs::create_dir_all(dir.join("deep/a/b/c")).unwrap();
    for name in ["x", "y", "z"] {
        std::fs::create_dir_all(dir.join("wide").join(name)).unwrap();
    }
    let top = Dir::open(&dir).unwrap();
    let copy = |from: &str, folders: usize, depth: usize| {
        let to = format!("{from}-{folders}-{depth}");
        let target = top.create_new_folder(OsStr::new(&to), 0o700).unwrap();
        let copying = Copying {
            limit: Some(Limit {
                files: 10,
                bytes: 1 << 20,
                folders,
                depth,
                what: "the test's",
            }),
            ..Copying::private(Links::Refuse)
        };
        top.folder(OsStr::new(from))
            .unwrap()
            .copy_into(&target, &copying)
            .map_err(|err| err.to_string())
    };
    assert_eq!(copy("deep", 10, 3).unwrap().folders, 3);
    assert!(
        copy("deep", 10, 2)
            .unwrap_err()
            .contains("more than 2 deep")
    );
    // Unlimited, a tree deeper than DEEPEST is refused all the same.
    let mut deep = dir.join("deeper");
    for _ in 0..=DEEPEST {
        deep.push("d");
    }
    std::fs::create_dir_all(&deep).unwrap();
    let target = top
        .create_new_folder(OsStr::new("deeper-copy"), 0o700)
        .unwrap();
    let err = top
        .folder(OsStr::new("deeper"))
        .unwrap()
        .copy_into(&target, &Copying::private(Links::Refuse))
        .unwrap_err();
    assert!(
        err.to_string()
            .contains(&format!("more than {DEEPEST} folders deep")),
        "{err}"
    );
    assert_eq!(copy("wide", 3, 1).unwrap().folders, 3);
    assert!(
        copy("wide", 2, 1)
            .unwrap_err()
            .contains("more than 2 folders")
    );
}

/// A tree removed whole, each link removed itself and never what it
/// names; one deeper than [`DEEPEST`] refused, never walked past it.
#[test]
fn remove_all_removes_links_never_their_targets_and_stops_at_its_depth() {
    let dir = temp("remove-all");
    let kept = dir.join("kept");
    std::fs::create_dir_all(kept.join("inside")).unwrap();
    std::fs::write(kept.join("inside/file"), "keep").unwrap();
    let tree = dir.join("tree");
    std::fs::create_dir_all(tree.join("a/b")).unwrap();
    std::fs::write(tree.join("a/b/file"), "x").unwrap();
    std::os::unix::fs::symlink(&kept, tree.join("a/to-folder")).unwrap();
    std::os::unix::fs::symlink(kept.join("inside/file"), tree.join("to-file")).unwrap();
    let top = Dir::open(&dir).unwrap();
    top.remove_all(OsStr::new("tree")).unwrap();
    assert!(!tree.exists());
    assert_eq!(
        std::fs::read_to_string(kept.join("inside/file")).unwrap(),
        "keep"
    );
    // A link itself, named: removed, its target kept.
    std::os::unix::fs::symlink(&kept, dir.join("link")).unwrap();
    top.remove_all(OsStr::new("link")).unwrap();
    assert!(kept.join("inside/file").exists());

    let mut deep = dir.join("deep");
    for _ in 0..=DEEPEST {
        deep.push("d");
    }
    std::fs::create_dir_all(&deep).unwrap();
    let err = top.remove_all(OsStr::new("deep")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
    assert!(dir.join("deep").exists());
}

/// Read-only folders of this user's, as Go's module cache leaves, are
/// emptied and removed; a part too deep is left, named by its path, and
/// everything beside it still goes.
#[test]
fn remove_all_empties_read_only_folders_and_names_what_it_leaves() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp("remove-read-only");
    let module = dir.join("tree/go/pkg/mod/example@v1");
    std::fs::create_dir_all(&module).unwrap();
    std::fs::write(module.join("go.mod"), "module example").unwrap();
    for folder in [module.clone(), dir.join("tree/go/pkg/mod")] {
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    let top = Dir::open(&dir).unwrap();
    top.remove_all(OsStr::new("tree")).unwrap();
    assert!(!dir.join("tree").exists());

    let mut deep = dir.join("mixed/deep");
    for _ in 0..DEEPEST {
        deep.push("d");
    }
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::create_dir_all(dir.join("mixed/beside")).unwrap();
    std::fs::write(dir.join("mixed/beside/file"), "x").unwrap();
    std::fs::write(dir.join("mixed/top"), "x").unwrap();
    let err = top.remove_all(OsStr::new("mixed")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
    let named = format!("mixed/deep{}", "/d".repeat(DEEPEST - 1));
    assert!(err.to_string().starts_with(&named), "{err}");
    assert!(dir.join("mixed/deep").exists());
    assert!(!dir.join("mixed/beside").exists() && !dir.join("mixed/top").exists());
}
