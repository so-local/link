//! One harness for all of an owner's agents on this machine, where the
//! machine says so (`agents.toml`'s `shared_harness`, off by default;
//! Linux only): Link Harness, in one sandbox, each agent behind its own
//! proxy as ever.
//!
//! Each agent's service runs its proxy alone (`lnk sandbox proxy
//! --hold`), and keeps in `~/.lnk/data/agents/shared-harnesses/<owner>/agents/<agent
//! id>.json` ([`Member`]) what the shared sandbox needs for it: its
//! sandbox as it would run alone, its proxy's folder and port, and the harness's
//! environment as it would have had it alone. The owner's harness (`lnk
//! agent shared <dir>`, [`serve`]) runs one sandbox holding every
//! member's together, reaching each member's proxy at its own port,
//! and `lnk-harness serve --agents`, which answers each agent on its
//! own port, with its own key, folders and tools. A member joining or
//! leaving starts the sandbox again with the members then, once the
//! turns it's answering finish ([`stop`]). One still running after that
//! is cut short, and recovered as after any crash. With no member left,
//! it ends.
//!
//! Only an agent its harness can share (`Contract::shared`), sandboxed
//! behind its proxy, reaching nothing but through it ([`sharing`]),
//! shares; any other keeps a sandbox of its own.

use link_format::WithinExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::bail;
use link_agent::adapter::Boxed;
use link_agent::settings::{self, Settings};
use link_harness_contract::harness::{Contract, Launch, SharedAgent, SharedAgents};
use link_sandbox_client::{At, Confinement, Net, Programs, Reached, Way};
use link_sandbox_contract::spec::{Localhost, Port, Spec};
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};

/// What the owner's harness serves an agent with, while the agent runs.
#[derive(Duplicate, Debug, PartialEq, Serialize, Deserialize)]
pub struct Member {
    /// Who it is, its folders, its endpoint's port, and its harness's
    /// environment.
    pub agent: SharedAgent,
    /// Its sandbox, as it would run alone, without its proxy.
    #[duplicate]
    pub confinement: Confinement,
    /// Its proxy's sockets, and the port its harness reaches it at.
    pub reached: Reached,
    /// The harness's command (`lnk-harness serve`).
    pub argv: Vec<String>,
}

link_files::specification!(Member, include_str!("shared/member.schema.toml"));

/// Whether the agent's harness, under `contract`, joins its
/// owner's one here, sandboxed as `boxed`, behind a proxy if `proxied`:
/// the machine says so, its harness can, and it reaches nothing but
/// through its proxy (no endpoint of its own; its spec opening no more
/// than the internet's hosts and its tools: no network as it is, no
/// machine of yours, no page, not your home; a local model on its proxy,
/// as on a Mac), so its sandbox is no more than its proxy and its
/// folders.
pub fn sharing(contract: &Contract, boxed: &Boxed, proxied: bool) -> bool {
    let spec = boxed.spec(false);
    let reads_home = spec
        .files
        .as_ref()
        .is_some_and(|files| files.read.iter().flatten().any(|p| p == "~"));
    link_agent::machine_settings::shared_harness().unwrap_or(false)
        && cfg!(target_os = "linux")
        && contract.shared
        && boxed.sandbox
        && proxied
        && boxed.folders.is_some()
        && spec.network.is_none()
        && spec.lan.is_none()
        && spec.browser.is_none()
        && spec.internet.as_ref().is_none_or(|i| i.tcp.is_none())
        && !reads_home
}

/// Keeps what the owner's harness serves agent `settings` with, its
/// sandbox (`confinement`) as it would run alone and its proxy's folder
/// and port, then starts the owner's harness if it isn't running: it
/// serves the agent once its next look finds it.
pub fn join(
    settings: &Settings,
    boxed: &Boxed,
    launch: &Launch,
    confinement: &Confinement,
    env: &std::collections::BTreeMap<String, String>,
    foreground: bool,
) -> anyhow::Result<()> {
    let owner = settings.owner()?;
    let dir = settings::shared_harness_of(&owner);
    let Net::Proxied { proxy: proxied, .. } = &confinement.net else {
        bail!("no proxy");
    };
    let port = launch
        .serve
        .as_ref()
        .and_then(|ports| ports.first().copied())
        .within("its harness serves no port")?;
    let mut alone = confinement.duplicate();
    alone.net = Net::Closed;
    let member = Member {
        agent: SharedAgent {
            name: settings.name.clone(),
            id: settings.key(),
            owner: owner.clone(),
            home: boxed.dir.clone(),
            files: settings.files(),
            agent: boxed.folders.is_some().then(|| settings.folder().agent),
            port,
            env: env.clone(),
        },
        confinement: alone,
        reached: Reached {
            dir: proxied.dir.clone(),
            port: proxied.listen.within("its proxy has no port")?,
        },
        argv: launch.argv.clone(),
    };
    let record = settings::shared_record_of(&owner, &settings.key());
    let records = record.parent().within("no folder")?;
    link_files::create_private_dir(records).within(format!("creating {}", records.display()))?;
    link_files::write_private(&record, &link_files::saved::print_json(&member)?)
        .within(format!("writing {}", record.display()))?;
    start(&dir, &owner, foreground)
}

/// Held while one of the owner's agents configures its harness and
/// joins the owner's harness in `dir`: two starting at once do it in
/// turn, never together. Let go when dropped, or when its process ends.
pub struct Configuring {
    _held: std::fs::File,
}

/// Waits until no other start of the owner's agents is configuring
/// their harness in `dir`, then holds it ([`Configuring`]).
pub fn configuring(dir: &Path) -> anyhow::Result<Configuring> {
    link_files::create_private_dir(dir).within(format!("creating {}", dir.display()))?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("configure.lock"))
        .within(format!("opening {}", dir.display()))?;
    lock.lock()
        .within(format!("waiting to configure in {}", dir.display()))?;
    Ok(Configuring { _held: lock })
}

/// Every agent the owner's harness in `dir` serves, by key, as their
/// records say now: those whose proxy runs, so an agent stopped without
/// a word (killed, or its machine's power cut) leaves it at the next look,
/// and those asleep for being idle, so one stopping or waking leaves the
/// sandbox as it is.
fn members(dir: &Path) -> anyhow::Result<std::collections::BTreeMap<String, Member>> {
    let mut members = recorded(dir)?;
    members.retain(|key, member| {
        link_agent::idle::asleep(key)
            || link_files::nofollow::connect(&member.reached.dir.join("out.sock")).is_ok()
    });
    Ok(members)
}

/// Whether every one of `members` is asleep: then nothing needs the
/// harness until one wakes, which starts it again.
fn all_asleep(members: &std::collections::BTreeMap<String, Member>) -> bool {
    !members.is_empty() && members.keys().all(|key| link_agent::idle::asleep(key))
}

/// Ends the owner's harness whose folder is `dir`: its service, so no
/// restart starts it again.
fn end_service(dir: &Path) {
    if let Some(owner) = owner_of(dir) {
        let (label, unit) = names(&owner);
        let file = match cfg!(target_os = "macos") {
            true => link_service::plist_path_of(&label),
            false => link_service::unit_path_of(&unit),
        };
        if link_service::managed() && file.exists() {
            link_service::stop_from_within(&file);
        }
    }
}

/// Every agent's record in `dir`, by key, running or not.
fn recorded(dir: &Path) -> anyhow::Result<std::collections::BTreeMap<String, Member>> {
    let mut members = std::collections::BTreeMap::new();
    let entries = match std::fs::read_dir(dir.join("agents")) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(members),
        Err(err) => return Err(err).within(format!("reading {}", dir.display())),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(key) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|key| settings::is_key(key))
        else {
            continue;
        };
        // One being written is read at the next look.
        let Ok(member) = std::fs::read(&path)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| link_files::saved::parse_json::<Member>(&bytes))
        else {
            continue;
        };
        members.insert(key.to_string(), member);
    }
    Ok(members)
}

/// What `members` are, named: the same members served the same way are
/// the same sandbox.
fn generation(members: &std::collections::BTreeMap<String, Member>) -> String {
    let json = serde_json::to_vec(members).unwrap_or_default();
    link_digest::sha256(&json)[..16].to_string()
}

/// `owner`'s harness's systemd unit ([`names`]).
pub fn unit_of(owner: &str) -> String {
    names(owner).1
}

/// `owner`'s harness's launchd label and systemd unit: by a hash of the
/// owner, since an owner's `@` would read as a unit template's.
fn names(owner: &str) -> (String, String) {
    let key = &link_digest::sha256(owner.as_bytes())[..12];
    (
        format!("link.shared.{key}"),
        format!("lnk-shared-{key}.service"),
    )
}

/// Starts `owner`'s harness, whose folder is `dir`, unless it runs: as a
/// service of its own where Link's services run and its agent is one,
/// else (in a terminal, or with no user's systemd) in a session of its
/// own. One runs at a time ([`serve`]'s lock).
fn start(dir: &Path, owner: &str, foreground: bool) -> anyhow::Result<()> {
    let program = std::env::current_exe().within("finding lnk-agent")?;
    let argv = vec![
        program.display().to_string(),
        "shared".into(),
        dir.display().to_string(),
    ];
    let log = dir.join("shared.log");
    let (label, unit) = names(owner);
    if link_service::managed() && !foreground {
        if link_service::running(&label, &unit) {
            return Ok(());
        }
        let mut env: std::collections::BTreeMap<String, String> =
            link_secretive::process::variables(|key| {
                matches!(key, "HOME" | "PATH" | "LANG" | "XDG_CONFIG_HOME")
                    || key.starts_with("LNK_")
            })
            .collect();
        env.insert("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into());
        let job = link_service::Job {
            argv: argv.clone(),
            env,
            workdir: dir.to_path_buf(),
            log: log.clone(),
        };
        match link_service::start(&label, &unit, &job) {
            Ok(()) => return Ok(()),
            // No service runs it: in a session of its own, as below.
            Err(_) => {
                let _ = link_service::stop(&label, &unit);
            }
        }
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .within(format!("opening {}", log.display()))?;
    let mut command = link_secretive::process::command(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // No agent's: a session of its own, and, forked once more with the
    // middle process gone, no agent's process's child, so the agent that
    // started it ending, or waited for, isn't its end.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut command, || {
            match libc::fork() {
                -1 => return Err(std::io::Error::last_os_error()),
                0 => {}
                _ => libc::_exit(0),
            }
            libc::setsid();
            Ok(())
        });
    }
    let mut middle = command.spawn().within("starting the shared harness")?;
    let _ = middle.wait();
    Ok(())
}

/// How often the owner's harness looks at who it serves.
const LOOK: Duration = Duration::from_millis(500);

/// How long a recorded agent's proxy may take to start before the
/// owner's harness, serving no other, ends.
const STARTING: Duration = Duration::from_secs(20);

/// How long a harness started for an agent waits for one ending to let
/// go: well past its last look, a moment before.
const ENDING: Duration = Duration::from_secs(2);

/// `lnk agent shared <dir>`: the owner's harness, serving the agents
/// `dir` names, again whenever they change, until none is left. One at
/// a time: another already serving is left to it.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a lock and its members, which nothing announces"
)]
pub fn serve(dir: &Path) -> anyhow::Result<()> {
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("serve.lock"))
        .within(format!("opening {}", dir.display()))?;
    // Another runs it: it takes this one's agent at its next look, and
    // never ends while an agent's record is new. One ending a moment ago,
    // between its last look and letting go, is waited for, and this one
    // serves.
    let waited = std::time::Instant::now();
    while lock.try_lock().is_err() {
        if waited.elapsed() > ENDING {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut none_since = std::time::Instant::now();
    loop {
        let members = members(dir)?;
        // A member just joining has a record before its proxy runs: given
        // a moment, then none left.
        if members.is_empty() && !recorded(dir)?.is_empty() && none_since.elapsed() < STARTING {
            std::thread::sleep(LOOK);
            continue;
        }
        if members.is_empty() {
            // Those left without a word (killed) are forgotten: their
            // next start records them again. One recorded while it waited
            // is kept, for the harness started for it.
            let mut fresh = false;
            for key in recorded(dir)?.keys() {
                let record = dir.join("agents").join(format!("{key}.json"));
                let stale = std::fs::metadata(&record)
                    .and_then(|m| m.modified())
                    .is_ok_and(|at| at.elapsed().unwrap_or_default() >= none_since.elapsed());
                match stale {
                    true => {
                        let _ = std::fs::remove_file(record);
                    }
                    false => fresh = true,
                }
            }
            // One recorded while it waited: waited for in turn.
            if fresh {
                none_since = std::time::Instant::now();
                continue;
            }
            // None left: its service ends, so no restart starts it again.
            end_service(dir);
            return Ok(());
        }
        // Every one asleep: none needs it until one wakes, which starts it.
        if all_asleep(&members) {
            end_service(dir);
            return Ok(());
        }
        let generation = generation(&members);
        let mut child = run(dir, &members, &generation)?;
        loop {
            std::thread::sleep(LOOK);
            if let Some(status) = child.try_wait()? {
                bail!("the shared harness ended ({status})");
            }
            let now = self::members(dir)?;
            if all_asleep(&now) {
                link_format::daemon::info!("Every agent served is asleep: ending until one wakes.");
                let _ = child.kill();
                let _ = child.wait();
                end_service(dir);
                return Ok(());
            }
            if self::generation(&now) != generation {
                link_format::daemon::info!("The agents served changed: starting again.");
                stop(&mut child, &agents_file(dir, &generation));
                none_since = std::time::Instant::now();
                break;
            }
        }
    }
}

/// The file naming the agents that the harness of `generation` serves.
fn agents_file(dir: &Path, generation: &str) -> PathBuf {
    dir.join(format!("agents-{generation}.json"))
}

/// Stops the shared harness for a change of its agents: asked first, by
/// removing the agents `file` it serves, so it takes no new connection,
/// the turns it's answering finish and the connections it has close, and
/// no member's turn is cut short; killed if it hasn't ended in the time
/// a harness has to stop ([`link_agent::waits::HarnessWaits::stop`]). A message arriving meanwhile waits for the new
/// harness, as one does for any start. Asked by its file, not a signal,
/// which would end its sandbox rather than reach it.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a draining child until it ends"
)]
fn stop(child: &mut std::process::Child, file: &Path) {
    if std::fs::remove_file(file).is_ok() {
        let asked = std::time::Instant::now();
        while asked.elapsed() < link_agent::waits::HarnessWaits::get().stop() {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The owner whose harness's folder is `dir`.
fn owner_of(dir: &Path) -> Option<String> {
    dir.file_name()?.to_str().map(str::to_string)
}

/// Runs `members`' harness, named `generation`: one sandbox with every
/// member's together, reaching each one's proxy, and the harness
/// serving each.
#[expect(
    clippy::disallowed_methods,
    reason = "starts the shared bridge with the environment it builds"
)]
fn run(
    dir: &Path,
    members: &std::collections::BTreeMap<String, Member>,
    generation: &str,
) -> anyhow::Result<std::process::Child> {
    let first = members.values().next().within("no agent")?;
    let agents = SharedAgents {
        agents: members.values().map(|m| m.agent.clone()).collect(),
    };
    let file = agents_file(dir, generation);
    link_files::write_private(&file, &link_files::saved::print_json(&agents)?)?;
    let confinement = together(dir, members, generation)?;
    let mut argv = first.argv.clone();
    argv.extend(["--agents".into(), file.display().to_string()]);
    let wrapped = link_sandbox_client::wrap(&confinement, &argv)?;
    let mut env = wrapped.env;
    env.insert("PATH".into(), "/usr/bin:/bin".into());
    env.insert("HOME".into(), dir.display().to_string());
    let tmp = dir.join("tmp");
    link_files::create_private_dir(&tmp)?;
    env.insert("TMPDIR".into(), tmp.display().to_string());
    Command::new(&wrapped.argv[0])
        .args(&wrapped.argv[1..])
        .env_clear()
        .envs(&env)
        .current_dir(dir)
        .spawn()
        .within("starting the shared harness's sandbox")
}

/// One sandbox for every member's: their folders written and read
/// together, every other agent's hidden, the ports each serves, each
/// proxy reached at its own port, the folder of this harness (its
/// agents' file, its HOME) written, and the programs each member runs.
fn together(
    dir: &Path,
    members: &std::collections::BTreeMap<String, Member>,
    generation: &str,
) -> anyhow::Result<Confinement> {
    let ports: Vec<Port> = members
        .values()
        .map(|m| Port::Number(m.agent.port))
        .collect();
    let spec = Spec {
        localhost: (!ports.is_empty()).then(|| Localhost {
            ports: Some(ports),
            ..Default::default()
        }),
        ..Default::default()
    };
    let name = format!("shared-{}", &generation[..12]);
    let mut confinement = Confinement::of(
        &spec,
        At {
            name: &name,
            dir,
            home: &link_files::home_dir(),
            path: None,
            way: Way::Reaching(members.values().map(|m| m.reached.clone()).collect()),
        },
    )?;
    confinement.write = vec![dir.to_path_buf()];
    confinement.tmp_prefix = members
        .values()
        .next()
        .and_then(|m| m.confinement.tmp_prefix.clone());
    let add = |to: &mut Vec<PathBuf>, paths: &[PathBuf]| {
        for path in paths {
            if !to.contains(path) {
                to.push(path.clone());
            }
        }
    };
    for member in members.values() {
        let alone = &member.confinement;
        add(&mut confinement.write, &alone.write);
        add(&mut confinement.read, &alone.read);
        add(&mut confinement.write_dot_files, &alone.write_dot_files);
        add(&mut confinement.hide, &alone.hide);
        add(&mut confinement.builds, &alone.builds);
        if let (Programs::Only(all), Programs::Only(its)) =
            (&mut confinement.commands, &alone.commands)
        {
            add(all, its);
        }
    }
    Ok(confinement)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, port: u16, proxy: u16) -> Member {
        Member {
            agent: SharedAgent {
                name: name.into(),
                id: format!("{name}0000000000")[..12].to_string(),
                owner: "me@here".into(),
                home: PathBuf::from(format!("/agents/{name}/home")),
                files: PathBuf::from(format!("/agents/{name}/files")),
                agent: None,
                port,
                env: Default::default(),
            },
            confinement: Confinement {
                name: format!("{name}-link"),
                write: vec![PathBuf::from(format!("/agents/{name}/home"))],
                write_dot_files: Vec::new(),
                read: vec![
                    PathBuf::from("/bin/lnk-harness"),
                    PathBuf::from(format!("/agents/{name}/files")),
                ],
                read_home: false,
                hide: vec![PathBuf::from("/agents")],
                tmp_prefix: None,
                terminal: false,
                commands: link_sandbox_client::Programs::Only(vec![PathBuf::from(
                    "/bin/lnk-harness",
                )]),
                developer_tools: false,
                builds: Vec::new(),
                net: Net::Closed,
            },
            reached: Reached {
                dir: PathBuf::from(format!("/net/{name}")),
                port: proxy,
            },
            argv: vec!["/bin/lnk-harness".into(), "serve".into()],
        }
    }

    /// A second start configuring waits until the first has joined.
    #[test]
    fn two_starts_configure_in_turn() {
        let dir = link_test_kit::TempDir::new("shared-configuring");
        let first = configuring(&dir).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let waiting = {
            let dir = dir.to_path_buf();
            std::thread::spawn(move || {
                let second = configuring(&dir).unwrap();
                tx.send(()).unwrap();
                drop(second);
            })
        };
        // Still held by the first: the second hasn't got it.
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        drop(first);
        rx.recv_timeout(Duration::from_secs(30)).unwrap();
        waiting.join().unwrap();
    }

    /// The shared sandbox holds each member's folders, once, reaches each
    /// one's proxy at its own port, and has no network of its own.
    #[test]
    fn the_sandbox_is_every_members_together() {
        let members = [
            ("ada".to_string(), member("ada", 20000, 20001)),
            ("bea".to_string(), member("bea", 20002, 20003)),
        ]
        .into_iter()
        .collect();
        let shared = together(Path::new("/shared"), &members, "0123456789abcdef").unwrap();
        assert_eq!(shared.name, "shared-0123456789ab");
        for path in ["/shared", "/agents/ada/home", "/agents/bea/home"] {
            assert!(shared.write.contains(&PathBuf::from(path)), "{shared:?}");
        }
        let harness = shared
            .read
            .iter()
            .filter(|p| *p == Path::new("/bin/lnk-harness"))
            .count();
        assert_eq!(harness, 1);
        assert_eq!(shared.hide, [PathBuf::from("/agents")]);
        let Net::Reaching { reached, ports } = &shared.net else {
            panic!("{:?}", shared.net)
        };
        assert_eq!(*ports, [20000, 20002].into_iter().collect());
        assert_eq!(
            reached.iter().map(|r| r.port).collect::<Vec<_>>(),
            [20001, 20003]
        );
        assert!(!shared.read_home);
        assert_eq!(
            shared.commands,
            link_sandbox_client::Programs::Only(vec![PathBuf::from("/bin/lnk-harness")]),
            "each member's programs, once"
        );
    }

    /// The same members are the same sandbox; one more, or one served
    /// otherwise, is another.
    #[test]
    fn a_change_of_members_is_a_new_sandbox() {
        let one: std::collections::BTreeMap<_, _> =
            [("ada".to_string(), member("ada", 20000, 20001))]
                .into_iter()
                .collect();
        let mut two = one.duplicate();
        assert_eq!(generation(&one), generation(&two));
        two.insert("bea".into(), member("bea", 20002, 20003));
        assert_ne!(generation(&one), generation(&two));
    }

    /// An owner's `@` never names a systemd template.
    #[test]
    fn its_service_is_named_by_a_hash_of_the_owner() {
        let (label, unit) = names("me@here");
        assert!(
            !unit.contains('@') && unit.starts_with("lnk-shared-"),
            "{unit}"
        );
        assert!(label.starts_with("link.shared."), "{label}");
    }
}
