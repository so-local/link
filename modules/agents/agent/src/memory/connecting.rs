//! A session of an agent's memory ([`connect`]), bound to its storage
//! bridge or its owner's service, started when it isn't running
//! ([`service`]).

use super::engine::{Network, Port};
use super::*;
use link_format::WithinExt;
use link_memory_tool_contract::declared::{Agent, Agents, MemoryServeManyArgs};

/// How long a session waits for its service to start: within the 30 s a
/// harness gives a tool.
const SERVICE_START: Duration = Duration::from_secs(20);

/// `lnk agent memory connect <dir> <agent>`: one session of agent
/// `key`'s memory, on stdio, with the service its owner's agents share
/// (in `dir`), started first when it isn't running. The agent is named
/// here, from the tool Link defined for it, never by the harness.
pub fn connect(dir: &Path, key: &str) -> anyhow::Result<()> {
    ensure!(dir.is_dir(), "no memory service in {}", dir.display());
    std::env::set_current_dir(working(dir)?)
        .within(format!("no memory service in {}", dir.display()))?;
    let stamps = stamps(dir)?;
    if !stamps.contains_key(key) {
        bail!("no memory of agent {key} here: lnk agent memory start sets it up");
    }
    let generation = current_generation(dir, stamps)?;
    let socket = socket_name(&generation);
    let hello = link_memory_tool_contract::Hello {
        agent: key.into(),
        bound: bound_of(dir, key)?,
        team: None,
    };
    let (mut writer, mut reader, welcome) = welcomed(
        || match std::os::unix::net::UnixStream::connect(&socket) {
            Ok(stream) => Ok(stream),
            Err(_) => start_service(dir, &generation),
        },
        &hello,
        SERVICE_START,
    )?;
    if !welcome.ok {
        bail!(
            "memory's service refused: {}",
            welcome.error.unwrap_or_default()
        );
    }
    // Each way until either ends: the session ends with the harness's.
    std::thread::spawn(move || {
        let _ = pass(&mut std::io::stdin().lock(), &mut writer);
        let _ = writer.shutdown(std::net::Shutdown::Write);
    });
    pass(&mut reader, &mut link_format::streams::protocol())
}

/// A session's two ends and the service's welcome, `hello` said on a
/// connection `reached` gives. The service ends a few seconds after its
/// last session, its socket gone only once it has; a session arriving as
/// it ends is taken by no one, and its connection closes unanswered.
/// That one is reached again, within `most`, finding the service still
/// ending or the socket gone and another started: never an error for an
/// agent whose harness started as the last session ended.
#[expect(
    clippy::disallowed_methods,
    reason = "waits out a service ending before reaching it again"
)]
fn welcomed(
    mut reached: impl FnMut() -> anyhow::Result<std::os::unix::net::UnixStream>,
    hello: &link_memory_tool_contract::Hello,
    most: Duration,
) -> anyhow::Result<(
    std::os::unix::net::UnixStream,
    std::io::BufReader<std::os::unix::net::UnixStream>,
    link_memory_tool_contract::Welcome,
)> {
    let said = serde_json::to_string(hello)?;
    let until = std::time::Instant::now() + most;
    loop {
        let stream = reached()?;
        match greet(stream, &said) {
            Ok(greeted) => return Ok(greeted),
            Err(err) if unanswered(&err) && std::time::Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(err) => return Err(err).within("memory's service didn't answer"),
        }
    }
}

/// [`welcomed`]'s one try: `said` written on `stream`, and the line it's
/// answered with read as a welcome.
fn greet(
    stream: std::os::unix::net::UnixStream,
    said: &str,
) -> std::io::Result<(
    std::os::unix::net::UnixStream,
    std::io::BufReader<std::os::unix::net::UnixStream>,
    link_memory_tool_contract::Welcome,
)> {
    let mut writer = stream.try_clone()?;
    writeln!(writer, "{said}")?;
    let mut reader = std::io::BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(std::io::ErrorKind::UnexpectedEof.into());
    }
    let welcome = serde_json::from_str(&line)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    Ok((writer, reader, welcome))
}

/// Whether `err` is a connection its service closed unanswered, as one
/// ending does.
fn unanswered(err: &std::io::Error) -> bool {
    use std::io::ErrorKind::{BrokenPipe, ConnectionAborted, ConnectionReset, UnexpectedEof};
    matches!(
        err.kind(),
        ConnectionReset | ConnectionAborted | BrokenPipe | UnexpectedEof
    )
}

/// Agent `key`'s binding for its memory, where its machine keeps what
/// memory reads (its conversations, its notes, its index's copy) off its
/// folder: its storage bridge, as its running bridge serves it to memory
/// ([`crate::bridge::memory_storage`]), and its key for this start, from
/// this machine's keyring. None where all of it is in its folder, or it
/// runs no storage bridge. Off its folder, with its bridge not running,
/// an error: never served from a folder that doesn't hold it.
fn bound_of(dir: &Path, key: &str) -> anyhow::Result<Option<link_memory_tool_contract::Bound>> {
    let Some(whose) = records(dir)?.remove(key).and_then(|r| r.whose) else {
        return Ok(None);
    };
    let storage = crate::bridge::Plan::load(&whose.name)
        .ok()
        .and_then(|plan| plan.storage);
    bound_for(key, whose, storage, &link_sandbox_binding::Keyring::here())
}

/// [`bound_of`] for agent `key`, `whose` it is, its storage bridge's
/// plan, and the keys of this machine's agents.
fn bound_for(
    key: &str,
    whose: Whose,
    storage: Option<crate::bridge::StoragePlan>,
    keys: &link_sandbox_binding::Keyring,
) -> anyhow::Result<Option<link_memory_tool_contract::Bound>> {
    use link_store_contract::storage::Collection;
    let Some(storage) = storage else {
        return Ok(None);
    };
    let elsewhere = [
        Collection::Conversations,
        Collection::Instructions,
        Collection::Memory,
    ]
    .into_iter()
    .any(|collection| storage.places.elsewhere(collection));
    if !elsewhere {
        return Ok(None);
    }
    // Its binding names the service it acts as, which memory proves its
    // calls with beside its ID.
    let binding = keys
        .bound(key)
        .filter(|binding| binding.identity.owner == whose.owner)
        .within(format!(
            "agent {key} isn't running: its memory is reached through its storage bridge"
        ))?;
    let socket = crate::bridge::memory_storage(&whose.owner, key);
    Ok(Some(link_memory_tool_contract::Bound {
        index: Default::default(),
        owner: whose.owner,
        service: binding.identity.service,
        key: binding.key,
        files: None,
        storage: Some(link_store_contract::storage::StorageAt {
            places: storage.places,
            socket: Some(socket),
        }),
        notes: None,
    }))
}

/// What `from` reads, written to `to` as it comes, until it ends: never
/// held back for more (`std::io::copy` may wait to fill a pipe's splice).
pub(super) fn pass(from: &mut impl std::io::Read, to: &mut impl Write) -> anyhow::Result<()> {
    let mut buffer = vec![0; 64 << 10];
    loop {
        let n = match from.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err.into()),
        };
        to.write_all(&buffer[..n])?;
        to.flush()?;
    }
}

/// Starts the service of `generation` in `dir`, one start at a time, and
/// a connection to it once it listens.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a starting service until it listens"
)]
fn start_service(dir: &Path, generation: &str) -> anyhow::Result<std::os::unix::net::UnixStream> {
    let socket = socket_name(generation);
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("start.lock"))
        .within("opening memory's start lock")?;
    lock.lock().within("waiting on memory's start lock")?;
    // Another session may have started it meanwhile.
    if let Ok(stream) = std::os::unix::net::UnixStream::connect(&socket) {
        return Ok(stream);
    }
    spawn_service(dir, generation)?;
    let started = std::time::Instant::now();
    loop {
        if let Ok(stream) = std::os::unix::net::UnixStream::connect(&socket) {
            return Ok(stream);
        }
        if started.elapsed() > SERVICE_START {
            bail!(
                "memory's service didn't start in {}s",
                SERVICE_START.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `lnk agent memory service <dir> <generation>`, apart from the session
/// that starts it, so it outlives the session: in a unit of its own where
/// systemd runs Link's services (else stopping the agent that started it
/// would stop every agent's), else in a session of its own.
fn spawn_service(dir: &Path, generation: &str) -> anyhow::Result<()> {
    let lnk = link_core::lnk();
    let program = lnk.get_program().to_string_lossy().into_owned();
    let args = [
        "agent".to_string(),
        "memory".into(),
        "service".into(),
        dir.display().to_string(),
        generation.to_string(),
    ];
    let log = dir.join("service.log");
    if cfg!(target_os = "linux")
        && link_service::managed()
        && std::env::var_os("XDG_RUNTIME_DIR").is_some()
    {
        let mut run = link_secretive::process::command("systemd-run");
        run.args(["--user", "--collect", "--quiet"])
            .arg(format!("--unit=lnk-memory-{generation}"))
            .arg(format!("--working-directory={}", working(dir)?.display()))
            .arg(format!(
                "--property=StandardOutput=append:{}",
                log.display()
            ))
            .arg(format!("--property=StandardError=append:{}", log.display()));
        run.args(link_service::setenv_args());
        run.arg("--").arg(&program).args(&args);
        if run
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            return Ok(());
        }
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .within(format!("opening {}", log.display()))?;
    let mut command = link_secretive::process::ours(&program);
    command
        .args(&args)
        .current_dir(working(dir)?)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // No agent's: a session of its own, and, forked once more with the
    // middle process gone, no session's process's child, so the agent
    // that started it ending, or waited for, isn't its end.
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
    let mut middle = command.spawn().within("starting memory's service")?;
    let _ = middle.wait();
    Ok(())
}

/// `lnk agent memory service <dir> <generation>`: the engine serving
/// every agent of one owner on this machine (as the records in `dir`
/// say, which must still be `generation`), confined to what each of
/// theirs would be together: their folders and the engine read, their
/// states and notes written, the network only for this machine's models,
/// served outside it, one each. It ends when the engine does, after its
/// last session.
pub fn service(dir: &Path, generation: &str) -> anyhow::Result<()> {
    let records = records(dir)?;
    if self::generation(&records) != generation {
        bail!("the agents memory serves changed: the next session starts it again");
    }
    let Some(first) = records.values().next() else {
        bail!("no agent's memory to serve");
    };
    let program = first.program.clone();
    if records.values().any(|r| r.program != program) {
        bail!("the agents here have different memory engines: each is served apart");
    }
    // Each model once, outside, for every agent naming it: all started
    // at once, and never waited for: the engine listens at once, and
    // searches by words until each answers at its address.
    let models: std::collections::BTreeSet<String> =
        records.values().filter_map(|r| r.model.clone()).collect();
    let servers: Vec<(String, Option<Served>)> = models
        .into_iter()
        .map(|model| {
            let served = Served::model(&model);
            (model, served)
        })
        .collect();
    let urls = addresses(&servers);
    let agents = Agents(
        records
            .iter()
            .map(|(key, record)| {
                let env = record
                    .model
                    .as_ref()
                    .map(|model| (MODEL_ENV.to_string(), urls[model].clone()))
                    .into_iter()
                    .chain(record.store.env.clone())
                    .collect();
                let agent = Agent {
                    argv: record.argv.clone(),
                    env,
                };
                (key.clone(), agent)
            })
            .collect(),
    );
    // Beside its working folder, read only to it.
    let runtime = link_files::layout::MemoryService::at(dir);
    let file = runtime.input(generation)?;
    link_files::write_private(&file, &link_files::saved::print_json(&agents)?)?;
    let run = working(dir)?;
    let writes: Vec<&Path> = records
        .values()
        .flat_map(|r| std::iter::once(r.state.as_path()).chain(r.notes.as_deref()))
        .collect();
    // Each agent's way to its storage bridge: the folder its socket is
    // in, made now, so a bridge started later is reached there; all of
    // them in the runtime folder's one for this owner, which the service
    // checks a binding against.
    let sockets = bridges_of(dir)?;
    let bridges: Vec<PathBuf> = records
        .iter()
        .filter(|(_, r)| r.whose.is_some())
        .filter_map(|(key, _)| {
            let dir = sockets.socket(key).parent()?.to_path_buf();
            link_files::create_private_dir(&dir).ok().map(|_| dir)
        })
        .collect();
    let reads: Vec<&Path> = records
        .values()
        .flat_map(|r| {
            std::iter::once(r.folder.as_path())
                .chain(r.reads.iter().map(PathBuf::as_path))
                .chain(r.programs.iter().map(PathBuf::as_path))
                .chain(r.store.drivers.iter().map(PathBuf::as_path))
        })
        .chain(bridges.iter().map(PathBuf::as_path))
        .chain(std::iter::once(file.as_path()))
        .collect();
    // Its models at their ports alone, served above; the machine's whole
    // network only for an agent whose engine finds runtimes itself.
    let network = match records.values().any(|r| r.network && r.model.is_none()) {
        true => Network::Host,
        false => Network::Models(
            urls.values()
                .filter_map(|url| embedding::port_of(url))
                .map(Port::At)
                .collect(),
        ),
    };
    let argv: Vec<String> = std::iter::once(program.display().to_string())
        .chain(serve_many(generation, &file, sockets.folder().to_path_buf()).argv())
        .collect();
    let status = link_core::lnk()
        .args(confined_writing(
            &program,
            &run,
            &writes,
            &reads,
            &network,
            &[],
            &argv,
        ))
        .current_dir(&run)
        .status()
        .within("running memory's service");
    drop(servers);
    let _ = std::fs::remove_file(&file);
    let status = status?;
    if !status.success() {
        bail!("memory's service ended: {status}");
    }
    Ok(())
}

/// Each model's address, as it was given it, whether or not it answers
/// yet: none where it couldn't be started.
fn addresses(servers: &[(String, Option<Served>)]) -> std::collections::BTreeMap<String, String> {
    servers
        .iter()
        .map(|(model, served)| {
            let url = served.as_ref().map(Served::url);
            (model.clone(), url.unwrap_or_default())
        })
        .collect()
}

/// Where the service in `dir` (`~/.lnk/data/agents/memory/<owner>`)
/// reaches its agents' storage bridges: its owner's folder of them in
/// the runtime folder, where each agent's bridge listens
/// ([`crate::bridge::memory_storage`]).
fn bridges_of(dir: &Path) -> anyhow::Result<link_files::layout::MemoryBridges> {
    let owner = dir
        .file_name()
        .and_then(|name| name.to_str())
        .within(format!("{} names no owner", dir.display()))?;
    Ok(link_files::layout::MemoryBridges::of(owner))
}

/// The service's line for `generation`, serving the agents in `agents`
/// with their storage bridges in `bridges`. A bound agent's storage bridge
/// is its own, here: no other socket. Its sessions come from this user,
/// named as its peer: the sandbox may run it as an identity of its own (a
/// Link box runs its controller's work as a fresh worker user), which
/// would otherwise refuse the agents it serves.
fn serve_many(generation: &str, agents: &Path, bridges: PathBuf) -> MemoryServeManyArgs {
    // SAFETY: getuid takes nothing and can't fail.
    let me = unsafe { libc::getuid() };
    MemoryServeManyArgs {
        socket: PathBuf::from(socket_name(generation)),
        agents: Some(agents.to_path_buf()),
        bridges: Some(bridges),
        cache: None,
        embed: None,
        embed_url: None,
        files_bridge: None,
        peer: Some(me.to_string()),
        socket_group: None,
        stay: false,
        storage_bridge: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    /// A model that never says where it listens, nor answers, holds up
    /// no service: its address is the engine's at once, the one it was
    /// started at, and the engine searches by words until it answers.
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a stand-in model server Served starts and stops itself"
    )]
    fn the_service_starts_without_waiting_for_its_models() {
        let silent = Served::start(|_| {
            let mut silent = Command::new("sh");
            silent.args(["-c", "cat"]);
            silent
        })
        .unwrap();
        let servers = vec![("qwen3-embedding".to_string(), Some(silent))];
        let urls = addresses(&servers);
        let silent = servers[0].1.as_ref().unwrap();
        assert_eq!(urls["qwen3-embedding"], silent.url());
        assert!(embedding::port_of(&urls["qwen3-embedding"]).is_some());
        // One that couldn't be started has none: the engine's words alone.
        assert_eq!(addresses(&[("x".into(), None)])["x"], "");
    }

    /// The service names the user who starts it as its peer, and shares
    /// its socket with no group: its sessions are that user's, whatever
    /// identity its sandbox runs it as.
    #[test]
    fn the_service_takes_the_sessions_of_the_user_who_starts_it() {
        let line = serve_many(
            "g1",
            Path::new("/m/agents.json"),
            PathBuf::from("/m/bridges"),
        );
        // SAFETY: getuid takes nothing and can't fail.
        let me = unsafe { libc::getuid() };
        assert_eq!(line.peer, Some(me.to_string()));
        assert_eq!(line.socket_group, None);
        assert_eq!(line.socket, PathBuf::from(socket_name("g1")));
    }

    /// The folder an owner's memory service is told (`--bridges`) is the
    /// one its agents' bridges listen in, in the runtime folder, never
    /// the service's own data folder: the service takes the socket its
    /// agent's bridge makes.
    #[test]
    fn the_service_is_told_the_folder_its_agents_bridges_listen_in() {
        let owner = "lnk-ci-3@lnk-smnhbew74mos";
        let dir = crate::settings::memory_service_of(owner);
        let told = bridges_of(&dir).unwrap();
        assert!(
            !told.folder().starts_with(&dir),
            "{}",
            told.folder().display()
        );
        let line = serve_many(
            "g1",
            Path::new("/m/agents.json"),
            told.folder().to_path_buf(),
        );
        let told = link_files::layout::MemoryBridges::at(line.bridges.unwrap());
        assert_eq!(
            told.socket("ceh9lefs1vp5"),
            crate::bridge::memory_storage(owner, "ceh9lefs1vp5")
        );
    }

    /// A session whose connection its service closes unanswered, as one
    /// ending does, is reached again and welcomed by the service there
    /// then; one closed every time is an error once its time is up.
    #[test]
    fn a_session_closed_unanswered_by_an_ending_service_is_reached_again() {
        let hello = link_memory_tool_contract::Hello {
            agent: "a1b2c3d4e5f6".into(),
            bound: None,
            team: None,
        };
        // The service's ends, kept open until the test ends.
        let mut served = Vec::new();
        let mut reached = 0;
        let (_, mut reader, welcome) = welcomed(
            || {
                reached += 1;
                let (ours, theirs) = UnixStream::pair()?;
                // The first, ending: closed with nothing said.
                if reached > 1 {
                    writeln!(&theirs, r#"{{"ok":true}}"#)?;
                    served.push(theirs);
                }
                Ok(ours)
            },
            &hello,
            Duration::from_secs(60),
        )
        .unwrap();
        assert!(welcome.ok);
        assert_eq!(reached, 2);
        // The hello went to the service that welcomed it.
        let mut asked = String::new();
        std::io::BufReader::new(&served[0])
            .read_line(&mut asked)
            .unwrap();
        assert_eq!(asked.trim_end(), serde_json::to_string(&hello).unwrap());
        drop(served);
        let mut rest = String::new();
        assert_eq!(reader.read_line(&mut rest).unwrap(), 0);

        let Err(failed) = welcomed(|| Ok(UnixStream::pair()?.0), &hello, Duration::ZERO) else {
            panic!("a service closing every session is an error");
        };
        assert!(
            format!("{failed:#}").contains("memory's service didn't answer"),
            "{failed:#}"
        );
    }

    /// An agent whose machine keeps what memory reads off its folder is
    /// bound through its storage bridge's memory socket, with its key for
    /// this start; one keeping all of it in its folder, or with no
    /// storage bridge, isn't; and one off its folder not running is
    /// refused, never served from its folder.
    #[test]
    fn memory_is_bound_through_the_storage_bridge_where_its_collections_are_elsewhere() {
        let _config = crate::tests::own_config();
        let dir = link_test_kit::TempDir::new("memory-bound");
        let keys = link_sandbox_binding::Keyring::at(&dir.join("keys"));
        let whose = Whose {
            owner: "me@box".into(),
            name: "main".into(),
        };
        let plan = |machine: serde_json::Value| crate::bridge::StoragePlan {
            notes_most: None,
            places: serde_json::from_value::<link_store_contract::storage::Supported>(machine)
                .unwrap()
                .resolve(&Default::default()),
            socket: dir.join("h/link-store.sock"),
            own: dir.join("f"),
            write: Vec::new(),
            memory: Vec::new(),
        };
        let here = plan(serde_json::json!({}));
        let sqlite = plan(serde_json::json!({"conversations": ["sqlite"]}));
        assert_eq!(
            bound_for("a1b2c3d4e5f6", whose.clone(), None, &keys).unwrap(),
            None
        );
        assert_eq!(
            bound_for("a1b2c3d4e5f6", whose.clone(), Some(here), &keys).unwrap(),
            None
        );
        // Not running: no key, refused.
        let refused =
            bound_for("a1b2c3d4e5f6", whose.clone(), Some(sqlite.clone()), &keys).unwrap_err();
        assert!(
            format!("{refused:#}").contains("isn't running"),
            "{refused:#}"
        );
        let binding = keys
            .bind(
                &link_sandbox_binding::Identity {
                    agent: "a1b2c3d4e5f6".into(),
                    service: "s1t2u3v4w5x6".into(),
                    owner: "me@box".into(),
                    teams: Vec::new(),
                },
                &link_secretive::Secret::from(link_store_contract::testing::TEST_KEY.to_string()),
            )
            .unwrap();
        let bound = bound_for("a1b2c3d4e5f6", whose, Some(sqlite.clone()), &keys)
            .unwrap()
            .expect("bound");
        assert_eq!(bound.owner, "me@box");
        assert_eq!(bound.service, "s1t2u3v4w5x6");
        assert_eq!(bound.key, binding.key);
        let storage = bound.storage.unwrap();
        assert_eq!(storage.places, sqlite.places);
        assert_eq!(
            storage.socket,
            Some(crate::bridge::memory_storage("me@box", "a1b2c3d4e5f6"))
        );
        // Another owner's agent of the ID: not this one's key.
        let theirs = Whose {
            owner: "you@box".into(),
            name: "main".into(),
        };
        assert!(bound_for("a1b2c3d4e5f6", theirs, Some(sqlite), &keys).is_err());
    }
}
