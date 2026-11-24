//! Executable restrictions: without Landlock no restricted target starts,
//! memory execution follows the policy, and `learn` finds refused programs.

use super::*;

/// An outer runtime can hide or deny Landlock even on a kernel that has
/// it. Inject that observation in this child alone, without changing the
/// test process or requiring the host to lack Landlock.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn landlock_probe_error(command: &mut Command, errno: u32) {
    // SAFETY: pre_exec runs only these async-signal-safe syscalls. The
    // filter is applied to the child and inherited by its descendants.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(command, move || {
            let filter = [
                libc::sock_filter {
                    code: 0x20,
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
                libc::sock_filter {
                    code: 0x15,
                    jt: 0,
                    jf: 1,
                    k: libc::SYS_landlock_create_ruleset as u32,
                },
                libc::sock_filter {
                    code: 0x06,
                    jt: 0,
                    jf: 0,
                    k: 0x0005_0000 | errno,
                },
                libc::sock_filter {
                    code: 0x06,
                    jt: 0,
                    jf: 0,
                    k: 0x7fff_0000,
                },
            ];
            let program = libc::sock_fprog {
                len: filter.len() as u16,
                filter: filter.as_ptr() as *mut _,
            };
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                || libc::syscall(libc::SYS_seccomp, 1u32, 0u32, &program) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// A version query refusal is deterministic here on hosts with and
/// without Landlock. Neither restricted policy may reach its target.
#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn unavailable_landlock_never_starts_a_restricted_target() {
    for (errno, observation) in [
        (libc::ENOSYS, "syscall unavailable"),
        (libc::EOPNOTSUPP, "support disabled"),
        (libc::EPERM, "probe denied"),
    ] {
        for listed in [false, true] {
            let world = World::new(&format!("landlock-refused-{errno}-{listed}"));
            let marker = world.work.join("target-ran");
            let mut args = vec!["confine", "--only"];
            if listed {
                args.extend(["--exec", "/bin/sh"]);
            }
            args.extend(["--", "/bin/sh", "-c", "echo ran > target-ran"]);
            let mut command = world.lnk(&args);
            landlock_probe_error(&mut command, errno as u32);
            let output = command.output().unwrap();
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(!output.status.success(), "{error}");
            assert!(
                error.contains(observation) && error.contains("target not started"),
                "{error}"
            );
            assert!(
                error.contains(if listed {
                    "only listed programs"
                } else {
                    "grants no programs"
                }),
                "{error}"
            );
            assert!(
                !marker.exists(),
                "target ran without executable restrictions"
            );
        }
    }
}

/// Run the probe only after a shell inside the sandbox has identified
/// itself. A refused launch cannot count as executable enforcement.
fn memory_exec(world: &World, how: &str) -> std::process::Output {
    let me = env!("CARGO_BIN_EXE_lnk-sandbox");
    let mut args = vec!["run", "--files.write", "."];
    args.extend([
        "--commands.allow",
        "/bin/sh",
        "--commands.allow",
        me,
        "--commands.allow",
        "/usr/bin/true",
    ]);
    let work = world.work.display().to_string();
    args.extend([
        "--",
        "/bin/sh",
        "-c",
        "printf 'probe-started\\n'; exec \"$@\"",
        "probe",
        me,
        "try-exec",
        "--how",
        how,
        "--payload",
        "/usr/bin/true",
        "--write",
        &work,
    ]);
    let output = world.lnk(&args).output().unwrap();
    assert!(
        String::from_utf8_lossy(&output.stdout).starts_with("probe-started\n"),
        "probe did not start inside the sandbox: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Restricted executable coverage requires a running, confined target.
/// The startup marker above is checked for every refusal separately.
#[test]
fn memory_exec_closes_with_restricted_programs() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    if !crate::landlock() {
        return;
    }
    if !Path::new("/usr/bin/true").exists() {
        let gate = Some(link_test_kit::gate::NATIVE);
        link_test_kit::gate::lacks("/usr/bin/true", "it isn't installed", gate);
        return;
    }
    let world = World::new("memexec-only");
    for how in ["memfd-huge", "memfd", "memfd-proc"] {
        let output = memory_exec(&world, how);
        assert!(
            !output.status.success(),
            "{how}: a memory executable ran with restricted programs"
        );
    }
}

/// A program `[commands]` doesn't list is refused by the kernel and named
/// by the sandbox, with its fix; `learn` runs the same command with every
/// program open and lists what to add, writing nothing.
#[test]
fn a_refused_program_is_named_and_learn_finds_it_first() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    if !crate::landlock() {
        return;
    }
    let world = World::new("learn");
    let script = "id >/dev/null; echo ran; exit 3";
    let output = world
        .lnk(&[
            "run",
            "--commands.allow",
            "bash",
            "--",
            "bash",
            "-c",
            script,
        ])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(3), "{err}");
    let id = std::fs::canonicalize("/usr/bin/id").unwrap();
    let named = ["/usr/bin/id".to_string(), id.display().to_string()]
        .iter()
        .any(|id| {
            err.contains(&format!(
                "lnk sandbox: refused {id}: add it to [commands] allow"
            ))
        });
    assert!(named, "{err}");
    let output = world
        .lnk(&[
            "learn",
            "--commands.allow",
            "bash",
            "--",
            "bash",
            "-c",
            script,
        ])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(3), "{out}");
    assert!(out.contains("ran\n"), "{out}");
    assert!(out.contains("Add them:\n\n  \"id\",\n"), "{out}");
    assert!(!out.contains("\"bash\""), "{out}");
}

/// What `learn` lists comes from its watch alone: a program writing to
/// every pipe it can open through `/proc/<pid>/fd` doesn't reach the
/// watch's, and lists nothing.
#[test]
fn a_program_cant_write_what_learn_lists() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("learnforge");
    let script = r#"for f in /proc/[0-9]*/fd/*; do
case "$(readlink "$f")" in pipe:*) (printf '/forged/prog\t/usr/bin/forged\n' > "$f") 2>/dev/null && echo "wrote $f";; esac
done; true"#;
    // Run by root too: the sandbox has none of root's capabilities.
    let output = world
        .lnk(&[
            "learn",
            "--commands.allow",
            "bash",
            "--",
            "bash",
            "-c",
            script,
        ])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Its own output (a pipe of this test's) may hold the line; the list
    // after it never does.
    let listed = out.rsplit_once("\n  It ran").map_or("", |(_, l)| l);
    assert!(!listed.is_empty() && !listed.contains("forged"), "{out}");
}

/// A build folder its caller declares (an install's own folder, which it
/// writes) runs what's written there; a program in another folder it
/// writes, undeclared, is still refused by the kernel.
#[test]
fn a_declared_build_folder_runs_what_it_writes_and_no_other_does() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    if !crate::landlock() {
        return;
    }
    if !Path::new("/usr/bin/true").exists() {
        let gate = Some(link_test_kit::gate::NATIVE);
        link_test_kit::gate::lacks("/usr/bin/true", "it isn't installed", gate);
        return;
    }
    let world = World::new("builds");
    let (staged, other) = (world.work.join("staged"), world.work.join("other"));
    for folder in [&staged, &other] {
        std::fs::create_dir_all(folder).unwrap();
        std::fs::copy("/usr/bin/true", folder.join("tool")).unwrap();
    }
    let mut commands = vec![PathBuf::from("/bin/sh"), staged.clone()];
    commands.extend(loader());
    let request = serde_json::json!({
        "confinement": {
            "name": "builds",
            "write": [world.work],
            "read": [],
            "commands": {"only": commands},
            "builds": [staged],
            "net": "closed",
        },
        "argv": [
            "/bin/sh",
            "-c",
            "\"$0\"/staged/tool && echo declared; \"$0\"/other/tool 2>/dev/null || echo refused",
            world.work,
        ],
    });
    let file = world.root.join("request.json");
    std::fs::write(&file, request.to_string()).unwrap();
    let output = world
        .lnk(&["wrap"])
        .stdin(std::fs::File::open(&file).unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let argv: Vec<String> = serde_json::from_value(reply["argv"].clone()).unwrap();
    let ran = link_test_kit::env::command(&argv[0], &world.home)
        .args(&argv[1..])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&ran.stdout);
    assert_eq!(
        out,
        "declared\nrefused\n",
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
}
