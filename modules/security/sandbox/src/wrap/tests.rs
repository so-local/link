use std::path::PathBuf;

use super::*;
use crate::tests::{bare, proxied};

#[test]
fn memory_runtime_mounts_and_rules_keep_records_closed() {
    let settings = link_files::installation_dir();
    let memory = link_files::layout::Hidden::SharedMemory.at(&settings);
    let owner = link_test_kit::TempDir::under(&memory, "memory-rules");
    let service = link_files::layout::MemoryService::at(&owner);
    let work = service.working();
    let input = service.input("0123456789abcdef").unwrap();
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(&input, "handoff").unwrap();
    let mut policy = bare(&[work.to_str().unwrap()], &[input.to_str().unwrap()]);
    // A broad home read must not expose the memory service's other files.
    policy.read_home = true;
    protected_closed(&policy, &settings, &link_files::try_link_dir().unwrap()).unwrap();
    let rules = link_sandbox_confine::Rules::from_resolved(&policy);
    assert!(rules.common.closed.contains(&memory));
    let safe = settings.join(&link_files::layout::LAYOUT.boundaries.hidden_safe);
    assert!(rules.common.hide.contains(&safe));

    let args = link_sandbox_confine::linux::bwrap::args(
        "/usr/bin/bwrap",
        &rules.common,
        &rules.linux,
        &["/bin/true".into()],
    );
    // Later mounts override earlier ancestors. Inspect the effective mount,
    // including the mask, rather than merely finding a grant somewhere.
    let mount = |path: &Path| {
        args.windows(2)
            .enumerate()
            .filter_map(|(at, pair)| {
                let destination = match pair[0].as_str() {
                    "--tmpfs" => &pair[1],
                    "--bind" | "--bind-try" | "--ro-bind" | "--ro-bind-try" => args.get(at + 2)?,
                    _ => return None,
                };
                path.starts_with(destination)
                    .then_some((pair[0].as_str(), destination))
            })
            .next_back()
            .unwrap()
    };
    assert_eq!(mount(&work), ("--bind-try", &work.display().to_string()));
    assert_eq!(
        mount(&input),
        ("--ro-bind-try", &input.display().to_string())
    );
    for path in [owner.join("agents/main.json"), owner.join("service.log")] {
        assert_eq!(mount(&path), ("--tmpfs", &memory.display().to_string()));
    }
    assert_eq!(mount(&safe), ("--tmpfs", &safe.display().to_string()));

    let profile = link_sandbox_confine::macos::seatbelt::profile(&rules.common, &rules.macos);
    let denied = |path: &Path| {
        format!(
            "(deny file-read* file-write* (subpath \"{}\"))",
            path.display()
        )
    };
    let closed = profile.find(&denied(&memory)).unwrap();
    assert!(profile.find("; read-home").unwrap() < closed);
    for (operation, path) in [
        ("file-read*", &work),
        ("file-write*", &work),
        ("file-read*", &input),
    ] {
        let allow = format!("(allow {operation} (subpath \"{}\"))", path.display());
        assert!(profile.rfind(&allow).unwrap() > closed, "{allow}");
    }
    assert!(!profile.contains(&format!(
        "(allow file-write* (subpath \"{}\"))",
        input.display()
    )));
    for path in [owner.join("agents/main.json"), owner.join("service.log")] {
        assert!(!profile.contains(&path.display().to_string()));
    }
    // Safe settings remain denied after the runtime grants are reopened.
    assert!(profile.rfind(&denied(&safe)).unwrap() > closed);
}

#[test]
fn memory_runtime_is_writable_but_its_handoff_records_and_logs_are_not() {
    let settings = Path::new("/home/me/.lnk");
    let home = Path::new("/home/me/Link");
    let folder = link_files::layout::Hidden::SharedMemory
        .at(settings)
        .join("me@here");
    let service = link_files::layout::MemoryService::at(&folder);
    let work = service.working();
    let input = service.input("0123456789abcdef").unwrap();
    for path in [&work, &work.join("memory.sock")] {
        protected_closed(&bare(&[path.to_str().unwrap()], &[]), settings, home).unwrap();
    }
    protected_closed(&bare(&[], &[input.to_str().unwrap()]), settings, home).unwrap();
    assert!(protected_closed(&bare(&[input.to_str().unwrap()], &[]), settings, home).is_err());
    for path in [
        folder.clone(),
        folder.join("agents/main.json"),
        folder.join("service.log"),
        folder.join("generation.json"),
        folder.join("agents-not-a-generation.json"),
        folder.join("bridges/main/storage.sock"),
        work.join("../agents/main.json"),
        settings.join("safe/agents/credentials"),
    ] {
        for policy in [
            bare(&[path.to_str().unwrap()], &[]),
            bare(&[], &[path.to_str().unwrap()]),
        ] {
            assert!(
                protected_closed(&policy, settings, home).is_err(),
                "{}",
                path.display()
            );
        }
    }
}

#[test]
fn links_settings_are_never_granted_nor_a_folder_holding_them_written() {
    let settings = Path::new("/home/me/.lnk");
    let home = Path::new("/home/me/Link");
    let closed = |confinement: &Confinement| protected_closed(confinement, settings, home);
    let with = bare;
    for refused in [
        with(&["/home/me"], &[]),
        with(&["/home/me/.lnk"], &[]),
        with(&["/"], &[]),
        with(&["/home/me/.lnk/safe/sandboxes"], &[]),
        with(&["/home/me/.lnk/exec/bin/lnk-x"], &[]),
        with(&["/home/me/.lnk/exec/harnesses/claude"], &[]),
        with(&[], &["/home/me/.lnk/safe"]),
        with(&[], &["/home/me/Link/Safe/Agents/main"]),
        with(&["/home/me/Link"], &[]),
        with(&["/usr/local/lib/lnk/lnk-secret"], &[]),
        with(&["/usr/local/lib"], &[]),
    ] {
        assert!(closed(&refused).is_err(), "{refused:?}");
    }
    for safe in link_files::layout::system_safes() {
        let path = safe.to_str().unwrap();
        assert!(closed(&with(&[], &[path])).is_err(), "{path}");
        assert!(closed(&with(&[path], &[])).is_err(), "{path}");
        assert!(
            closed(&with(&[safe.parent().unwrap().to_str().unwrap()], &[])).is_err(),
            "{path}"
        );
    }
    // Read around them, write beside them.
    closed(&with(
        &["/home/me/code"],
        &["/home/me", "/home/me/Link", "/home/me/.lnk"],
    ))
    .unwrap();
    // A program of Link's, read by the file.
    closed(&with(
        &[],
        &[
            "/home/me/.lnk/exec/bin/lnk-x",
            "/home/me/.lnk/exec/harnesses/claude",
        ],
    ))
    .unwrap();
    // A harness's own home, and what's in it, but not the folder of
    // them, nor the rest of its agent's.
    let homes = "/home/me/.lnk/data/agents/main/harnesses";
    closed(&with(&[&format!("{homes}/hermes")], &[])).unwrap();
    closed(&with(&[], &[&format!("{homes}/hermes/.env")])).unwrap();
    closed(&with(&["/home/me/.lnk/data"], &[])).unwrap();
    for refused in [
        with(&["/home/me/.lnk/safe/agents/main"], &[]),
        with(&["/home/me/.lnk/safe/agents/main/proxy"], &[]),
    ] {
        assert!(closed(&refused).is_err(), "{refused:?}");
    }
}

/// Every path a confinement names is plain, the folders of its
/// network's sockets too: a proxy's, a loopback's, other proxies'.
#[test]
fn every_path_it_names_is_plain() {
    assert!(check_paths(&bare(&["/w"], &["/r"])).is_ok());
    assert!(check_paths(&bare(&["/w\nx"], &[])).is_err());
    let bad = PathBuf::from("/n\r(allow default)");
    let looped = Confinement {
        net: Net::Loopback {
            ports: BTreeSet::new(),
            dir: bad.clone(),
        },
        ..bare(&[], &[])
    };
    let reaching = Confinement {
        net: Net::Reaching {
            reached: vec![link_sandbox_client::Reached {
                dir: bad.clone(),
                port: 40781,
            }],
            ports: BTreeSet::new(),
        },
        ..bare(&[], &[])
    };
    let mut proxied = proxied();
    if let Net::Proxied { proxy, .. } = &mut proxied.net {
        proxy.dir = bad.clone();
    }
    for confinement in [looped, reaching, proxied] {
        let err = check_paths(&confinement).unwrap_err().to_string();
        assert!(err.contains("control character"), "{err}");
    }
}
