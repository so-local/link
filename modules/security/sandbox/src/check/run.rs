//! Running the probes: the bait set up, each probe run under its
//! confinement, and everything cleaned up.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use link_sandbox_client::{Confinement, Programs};
use link_sandbox_confine::Os;
use link_secretive::Duplicate;

use super::bait::{abstract_socket, bypass_lib, bypass_payload, keychain_item, own_address};
use super::dot_files::{dot_files, moved_git};
use super::probes::refused;
use super::scripts::{PROGRAM_RAN, TARGET_TAKEN};
use super::{Bait, CHECK_FILE, Expect, Expected, Probe, Verdict, disp, probes, settings_write};

/// The descriptor each probe is started holding, a pipe's: what a caller
/// leaves open (a CI runner's pipe to its worker, say), closed inside.
const LEFT_OPEN: i32 = 9;

/// Sets up the bait, runs every probe under `policy`, and cleans up.
pub fn run(
    policy: &crate::spec::Resolved,
    expected: &Expected,
) -> anyhow::Result<Vec<(Probe, Verdict)>> {
    let policy: &Confinement = policy;
    crate::check_name(&policy.name)?;
    let proxied = crate::proxy_of(policy);
    // Its probes are refused on purpose, and a new port each time: a wire
    // its earlier runs paused ([`link_sandbox_proxy::wire::Watch`]) is opened again.
    if proxied.is_some() {
        let _ = link_sandbox_proxy::wire::resume(&link_sandbox_proxy::wire::dir()?, &policy.name);
    }
    let home = link_files::home_dir();
    let secret = link_files::random_token()?;
    // Random, never the process's id: what this run names in the folders
    // the sandbox writes can't be guessed and planted before it.
    let tag = u32::from_str_radix(&secret[..8], 16)?;
    let canary = home.join(format!("lnk-check-{tag}"));
    std::fs::write(&canary, &secret)?;
    let settings = link_files::layout::Runtime::Checks
        .path()
        .join(format!("lnk-check-{tag}"));
    link_files::write_private(&settings, secret.as_bytes())?;
    let hidden: Vec<(PathBuf, PathBuf)> = policy
        .hide
        .iter()
        .filter(|folder| folder.is_dir())
        .map(|folder| (folder.clone(), folder.join(format!("lnk-check-{tag}"))))
        .filter(|(_, file)| link_files::write_private(file, secret.as_bytes()).is_ok())
        .collect();
    let reads: Vec<(PathBuf, PathBuf, bool)> = expected
        .reads
        .iter()
        .map(|folder| (folder, true))
        .chain(expected.unread.iter().map(|folder| (folder, false)))
        .filter(|(folder, _)| folder.is_dir())
        .map(|(folder, read)| {
            (
                folder.clone(),
                folder.join(format!("lnk-check-{tag}")),
                read,
            )
        })
        .filter(|(_, file, _)| link_files::write_private(file, secret.as_bytes()).is_ok())
        .collect();
    let elsewhere = PathBuf::from(format!("/var/tmp/lnk-check-{tag}"));
    // Best effort: a machine without /var/tmp skips nothing, the probe
    // just finds nothing.
    let _ = std::fs::write(&elsewhere, &secret);
    let listener = TcpListener::bind("0.0.0.0:0")?;
    let (abstract_socket, _abstract_listener) = abstract_socket(tag);
    let mut helper = link_secretive::process::command("sleep")
        .arg("60")
        .env("LNK_CHECK_SECRET", &secret)
        .stdout(Stdio::null())
        .spawn()?;
    let dot_files = dot_files(policy, expected)?;
    let bait = Bait {
        dot_files,
        tag,
        write: home.join(format!("lnk-check-write-{tag}")),
        settings,
        hidden,
        reads,
        elsewhere,
        writable: policy.write.clone(),
        port: listener.local_addr()?.port(),
        lan: own_address(),
        pid: helper.id(),
        keychain: keychain_item(tag),
        python: cfg!(target_os = "linux")
            && ["/usr/bin/python3", "/bin/python3", "/usr/local/bin/python3"]
                .iter()
                .any(|p| Path::new(p).is_file()),
        abstract_socket,
        ports: link_sandbox_confine::ports(&policy.net)
            .into_iter()
            .filter(|p| !proxied.is_some_and(|x| x.serve.contains(p)))
            .collect(),
        sandbox: std::env::current_exe()?.canonicalize()?,
        models: match proxied {
            Some(proxy) => proxy.models().into_iter().collect(),
            None => Vec::new(),
        },
        payload: bypass_payload(),
        loader: link_sandbox_confine::linux::landlock::loaders()
            .into_iter()
            .next(),
        lib: bypass_lib(),
        lan_hosts: proxied.map(|p| p.lan.clone()).unwrap_or_default(),
        dns: cfg!(target_os = "linux") && proxied.is_some_and(|p| !p.tcp.is_empty()),
        internet: proxied.is_some_and(|p| {
            !p.no_internet
                && (p.hosts.is_empty()
                    || link_sandbox_contract::net::host_allowed("1.1.1.1", &p.hosts))
        }),
        home,
        secret,
        canary,
    };
    let gits: Vec<(PathBuf, bool)> = bait
        .dot_files
        .iter()
        .map(|dots| {
            (
                dots.folder.clone(),
                dots.folder.join(".git").symlink_metadata().is_ok(),
            )
        })
        .collect();
    let result = run_probes(policy, &bait, expected);
    for (folder, was_there) in &gits {
        put_back_git(folder, tag, *was_there);
    }
    if let Some(service) = &bait.keychain {
        let _ = link_secretive::process::command("security")
            .args(["delete-generic-password", "-s", service])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = helper.kill();
    let _ = helper.wait();
    let written = bait
        .writable
        .iter()
        .map(|dir| dir.join(CHECK_FILE))
        .chain(bait.hidden.iter().map(|(_, file)| file.clone()))
        .chain(bait.reads.iter().map(|(_, file, _)| file.clone()))
        .chain([settings_write(&bait.settings)]);
    for path in [&bait.canary, &bait.write, &bait.settings, &bait.elsewhere]
        .map(PathBuf::clone)
        .into_iter()
        .chain(written)
    {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// A `.git` the probe moved aside, back, should the sandbox have let it
/// go but not come back: only one that `was_there` before the probes,
/// and a folder, never a link. The folder is the sandbox's to write, so
/// anything else at the aside's name is what it planted, which put in
/// place would run outside the sandbox at your next git command.
pub(super) fn put_back_git(folder: &Path, tag: u32, was_there: bool) {
    let aside = moved_git(folder, tag);
    let git = folder.join(".git");
    if was_there
        && aside.symlink_metadata().is_ok_and(|m| m.is_dir())
        && git.symlink_metadata().is_err()
    {
        let _ = std::fs::rename(&aside, &git);
    }
}

fn run_probes(
    policy: &Confinement,
    bait: &Bait,
    expected: &Expected,
) -> anyhow::Result<Vec<(Probe, Verdict)>> {
    let os = link_sandbox_confine::here()?.os();
    let linux = os == Os::Linux;
    // Its first folder is its home, as a harness's is.
    let home = policy
        .write
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("/"));
    let mut results = Vec::new();
    let hosts = crate::proxy_of(policy)
        .map(|p| p.hosts.as_slice())
        .unwrap_or_default();
    let unlisted = (!hosts.is_empty()).then(|| {
        ["example.com", "example.org", "example.net"]
            .into_iter()
            .find(|h| !link_sandbox_contract::net::host_allowed(h, hosts))
    });
    let unlisted = unlisted.flatten();
    // The probes run this program inside (the macOS service lookups).
    let mut policy = policy.duplicate();
    policy.read.push(bait.sandbox.clone());
    // The probes are shell scripts using the system's tools: they run
    // with any program, and one more tries a program it doesn't list.
    let commands = std::mem::replace(&mut policy.commands, Programs::Any).listed();
    let mut all: Vec<(Probe, Confinement)> = probes(&policy, bait, os, unlisted)
        .into_iter()
        .map(|probe| (probe, policy.duplicate()))
        .collect();
    let probe = |what: String, script, expect, arg: String| Probe {
        what,
        script,
        expect,
        lands: None,
        arg: Some(arg),
        path: None,
        required: expect == Expect::Allowed,
    };
    for target in &expected.reaches {
        all.push((
            probe(
                format!("reach {target}"),
                TARGET_TAKEN,
                Expect::Allowed,
                target.clone(),
            ),
            policy.duplicate(),
        ));
    }
    for target in &expected.refuses {
        all.push((refused(target), policy.duplicate()));
    }
    // Run as the policy would: only what it lists, and the probe's shell.
    let listing = |program: &PathBuf| {
        let mut only = policy.duplicate();
        only.commands = match &commands {
            Some(programs) => Programs::Only(
                programs
                    .iter()
                    .cloned()
                    .chain([link_sandbox_confine::resolve(Path::new("/bin/bash"))])
                    .collect(),
            ),
            None => Programs::Any,
        };
        (only, program.display().to_string())
    };
    for (programs, expect) in [
        (&expected.runs, Expect::Allowed),
        (&expected.refused_programs, Expect::Blocked),
    ] {
        for program in programs {
            let (only, arg) = listing(program);
            all.push((probe(format!("run {arg}"), PROGRAM_RAN, expect, arg), only));
        }
    }
    if let Some(programs) = commands {
        let bash = link_sandbox_confine::resolve(Path::new("/bin/bash"));
        let path = std::ffi::OsString::from("/usr/bin:/bin");
        let other = ["id", "uname", "whoami", "date", "env"]
            .into_iter()
            .filter_map(|name| link_files::find_program(name, &path))
            .map(|p| link_sandbox_confine::resolve(&p))
            .find(|p| !link_sandbox_confine::programs::listed(&programs, p));
        if let Some(other) = other {
            let mut only = policy.duplicate();
            only.commands =
                Programs::Only(programs.iter().cloned().chain([bash.clone()]).collect());
            all.push((
                Probe {
                    what: format!("run a program it doesn't list ({})", other.display()),
                    script: r#""$LNK_PROBE_ARG" >/dev/null 2>&1"#,
                    expect: Expect::Blocked,
                    lands: None,
                    arg: Some(other.display().to_string()),
                    path: None,
                    required: false,
                },
                only,
            ));
        }
        // The ways around the list a program can write and run its own
        // code: a memory file, a file it wrote run through `/proc/self/fd`,
        // the loader, and `LD_PRELOAD`. Each tried by `lnk-sandbox` from
        // inside, which is always allowed to run. Memory and `/proc/self/fd`
        // are closed where a spec lists programs (a memory file through
        // `/proc/self/fd` on Linux 6.3 and newer); the loader and
        // `LD_PRELOAD` can't be, a known gap.
        if linux && let Some(payload) = &bait.payload {
            let write = bait.writable.first();
            let listed: Vec<PathBuf> = programs
                .iter()
                .cloned()
                .chain([bash, payload.clone()])
                .collect();
            let gap_ldso = Expect::Gap(
                "the dynamic loader runs any file it can read; it must stay runnable for \
                 every dynamically linked program",
            );
            let gap_preload = Expect::Gap(
                "LD_PRELOAD loads a library from a write folder into a listed program; it is \
                 cleared at the start, but a program can set it for a child",
            );
            // A sealing listener keeps memory files non-executable. If
            // either capability is unavailable, the base filter refuses
            // their creation instead. Neither route is a known-open gap.
            let ways: &[(&str, &str, Expect, bool)] = &[
                (
                    "run a program copied into memory (memfd_create, execveat)",
                    "memfd",
                    Expect::Blocked,
                    false,
                ),
                (
                    "run a memory program through /proc/self/fd",
                    "memfd-proc",
                    Expect::Blocked,
                    false,
                ),
                (
                    "run a memory program of huge pages through /proc/self/fd",
                    "memfd-huge",
                    Expect::Blocked,
                    false,
                ),
                (
                    "run a file it wrote through /proc/self/fd",
                    "written-proc",
                    Expect::Blocked,
                    true,
                ),
                (
                    "run a file it wrote with the loader (ld.so ./x)",
                    "ldso",
                    gap_ldso,
                    true,
                ),
                (
                    "load a library it wrote into a listed program (LD_PRELOAD)",
                    "preload",
                    gap_preload,
                    true,
                ),
            ];
            for (what, how, expect, needs_write) in ways {
                if *how == "preload" && bait.lib.is_none() {
                    continue;
                }
                if *needs_write && write.is_none() {
                    continue;
                }
                let mut only = policy.duplicate();
                only.commands = Programs::Only(listed.clone());
                all.push((
                    Probe {
                        what: (*what).to_string(),
                        script: r#""$LNK_PROBE_SANDBOX" sandbox try-exec --how "$LNK_PROBE_ARG" \
                            --payload "$LNK_PROBE_PAYLOAD" \
                            ${LNK_PROBE_WRITE:+--write "$LNK_PROBE_WRITE"} \
                            ${LNK_PROBE_LOADER:+--loader "$LNK_PROBE_LOADER"} \
                            ${LNK_PROBE_LIB:+--lib "$LNK_PROBE_LIB"}"#,
                        expect: *expect,
                        lands: None,
                        arg: Some((*how).to_string()),
                        path: None,
                        required: false,
                    },
                    only,
                ));
            }
        }
    }
    let wire = link_sandbox_proxy::wire::dir()?;
    // A pipe each probe is started holding, as its descriptor
    // [`LEFT_OPEN`]: what a caller leaves open, which no program inside
    // should reach. Never read: a write that got through only fills it.
    let (_left, left_open) = std::io::pipe()?;
    for (probe, policy) in all {
        let policy = &policy;
        // Asking for a metadata service pauses the wire, as it should:
        // each probe starts with it open, so a refusal is its own call's.
        if crate::proxy_of(policy).is_some() {
            let _ = link_sandbox_proxy::wire::resume(&wire, &policy.name);
        }
        let argv = ["/bin/bash".to_string(), "-c".into(), probe.script.into()];
        let wrapped = crate::wrap_resolved(policy, &argv)?;
        let argv = wrapped.argv;
        let target = probe.lands.as_deref().unwrap_or(Path::new(""));
        // Only the bait it's given: none of this process's secrets.
        let mut command = link_secretive::process::command(&argv[0]);
        let left = std::os::fd::AsRawFd::as_raw_fd(&left_open);
        // SAFETY: dup2 and fcntl are async-signal-safe; the pipe is open.
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(&mut command, move || {
                // Already LEFT_OPEN, it would be closed at exec unless kept.
                let kept = match left == LEFT_OPEN {
                    true => libc::fcntl(left, libc::F_SETFD, 0),
                    false => libc::dup2(left, LEFT_OPEN),
                };
                match kept {
                    -1 => Err(std::io::Error::last_os_error()),
                    _ => Ok(()),
                }
            });
        }
        let status = command
            .args(&argv[1..])
            .env_clear()
            .envs([
                ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin".to_string()),
                ("LANG", "en_US.UTF-8".into()),
                ("HOME", home.display().to_string()),
                ("LNK_PROBE_HOME", bait.home.display().to_string()),
                ("LNK_PROBE_SECRET", bait.secret.clone()),
                ("LNK_PROBE_CANARY", bait.canary.display().to_string()),
                ("LNK_PROBE_TARGET", target.display().to_string()),
                ("LNK_PROBE_SETTINGS", bait.settings.display().to_string()),
                ("LNK_PROBE_ELSEWHERE", bait.elsewhere.display().to_string()),
                ("LNK_PROBE_PORT", bait.port.to_string()),
                (
                    "LNK_PROBE_LAN",
                    bait.lan.map(|ip| ip.to_string()).unwrap_or_default(),
                ),
                ("LNK_PROBE_PID", bait.pid.to_string()),
                (
                    "LNK_PROBE_UNLISTED",
                    unlisted.unwrap_or_default().to_string(),
                ),
                ("LNK_PROBE_ARG", probe.arg.clone().unwrap_or_default()),
                ("LNK_PROBE_SANDBOX", bait.sandbox.display().to_string()),
                (
                    "LNK_PROBE_PATH",
                    probe
                        .path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                ),
                (
                    "LNK_PROBE_ABSTRACT",
                    bait.abstract_socket.clone().unwrap_or_default(),
                ),
                (
                    "LNK_PROBE_KEYCHAIN_ITEM",
                    bait.keychain.clone().unwrap_or_default(),
                ),
                (
                    "LNK_PROBE_PAYLOAD",
                    bait.payload.as_ref().map(disp).unwrap_or_default(),
                ),
                (
                    "LNK_PROBE_WRITE",
                    bait.writable.first().map(disp).unwrap_or_default(),
                ),
                (
                    "LNK_PROBE_LOADER",
                    bait.loader.as_ref().map(disp).unwrap_or_default(),
                ),
                (
                    "LNK_PROBE_LIB",
                    bait.lib.as_ref().map(disp).unwrap_or_default(),
                ),
            ])
            .envs(wrapped.env)
            .current_dir(&home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        let reached = match &probe.lands {
            Some(path) => path.is_file(),
            None => status.success(),
        };
        let verdict = probe.verdict(reached);
        results.push((probe, verdict));
    }
    Ok(results)
}
