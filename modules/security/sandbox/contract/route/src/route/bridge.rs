//! A route's bridge as it runs: one per module per machine's user, on a
//! socket in the private runtime folder (`sandbox/bridges/<module>.sock`),
//! started by the first proxy whose call needs it ([`reach`]) and gone
//! once no call has come for a while, at once when no one is bound
//! to prove one, or once an upgrade has replaced its program and no call
//! is in flight ([`serve`]). Where systemd runs Link's services, it's a
//! user unit of its own (`lnk-bridge-<module>`), so no agent's service
//! holds it: restarting one agent leaves every other agent's tools as
//! they were. A lock beside its socket keeps it to one. What it holds for its calls lives in it,
//! shared by every sandbox that calls it, each told apart by its proof.
//!
//! Started, it's handed its context ([`Bridge::start`]: its socket, the
//! keyring its calls are proved against, Link's settings and runtime), never left to
//! find it from the environment it starts in: a proxy runs with its
//! program's, whose `HOME` is a harness's own folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde::de::DeserializeOwned;

use super::Bridge;
use link_sandbox_binding::{Identity, Keyring, Limits};

/// How long a proxy waits for a bridge it started to listen.
pub const START_WAIT: Duration = Duration::from_secs(10);

/// The most a bridge's log, beside its socket, grows before it's kept
/// once as `<module>.log.1` at the next start.
const MAX_LOG: u64 = 1024 * 1024;

/// How often a bridge looks whether it's idle, or its socket is gone.
const LOOK_EVERY: Duration = Duration::from_secs(5);

/// What a bridge is started with: what [`Bridge::start`] says, read from
/// the bridge program's generated arguments, declared in its
/// `actions.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    /// Its socket.
    pub socket: PathBuf,
    /// The keyring its calls are proved against.
    pub keys: PathBuf,
    /// Link's settings, whose user it serves.
    pub settings: PathBuf,
    /// The explicit runtime root, independent of the proxy's environment.
    pub runtime: PathBuf,
}

/// Serves a bridge as `started` says, each stream call ([`super::Call`]
/// its action) handed to `handle` with who called, until no call has
/// come for `idle`, or none is running and its keyring binds no one (the
/// last agent stopped, the last sandbox ended), or its socket is no
/// longer its own: then it returns.
/// Returns at once, serving nothing, when another serves the socket.
pub async fn serve<F, Fut>(
    started: &Started,
    limits: &Limits,
    idle: Duration,
    handle: F,
) -> anyhow::Result<()>
where
    F: Fn(Identity, super::Call, link_sandbox_bridge::stream::Stream) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    serve_as(started, limits, idle, handle).await
}

/// [`serve`], its actions of any type.
#[expect(
    clippy::disallowed_methods,
    reason = "the bridge's own period of looking"
)]
pub async fn serve_as<A, F, Fut>(
    started: &Started,
    limits: &Limits,
    idle: Duration,
    handle: F,
) -> anyhow::Result<()>
where
    A: DeserializeOwned,
    F: Fn(Identity, A, link_sandbox_bridge::stream::Stream) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let socket = &started.socket;
    let dir = socket
        .parent()
        .with_context(|| format!("{} is no socket's path", socket.display()))?;
    link_files::create_private_dir(dir).with_context(|| format!("creating {}", dir.display()))?;
    let Some(_lock) = Lock::take(&socket.with_extension("lock"))? else {
        // Another bridge serves it.
        return Ok(());
    };
    let _ = std::fs::remove_file(socket);
    let listener = crate::socket::listen_async(socket, crate::socket::Given::default())?;
    let ours = identity_of(socket)?;
    // Its program as it started: replaced by an upgrade, it ends once
    // idle, and the next call starts the new one.
    let program = std::env::current_exe()
        .ok()
        .and_then(|exe| Some((identity_of(&exe).ok()?, exe)));
    let keyring = Keyring::at(&started.keys);
    let keys: Arc<dyn link_sandbox_binding::Keys> = Arc::new(keyring.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let last = Arc::new(std::sync::Mutex::new(Instant::now()));
    let stop = AtomicBool::new(false);
    let served =
        link_sandbox_bridge::serve_streams(listener, keys, limits, &stop, |who, action, stream| {
            let (calls, last) = (calls.clone(), last.clone());
            calls.fetch_add(1, Ordering::SeqCst);
            let call = handle(who, action, stream);
            async move {
                let done = call.await;
                calls.fetch_sub(1, Ordering::SeqCst);
                *last.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                done
            }
        });
    let watched = async {
        loop {
            tokio::time::sleep(LOOK_EVERY.min(idle)).await;
            let replaced = program
                .as_ref()
                .is_some_and(|(was, exe)| identity_of(exe).ok() != Some(*was));
            let quiet = ends(
                calls.load(Ordering::SeqCst),
                !keyring.binds_no_one(),
                last.lock().unwrap_or_else(|e| e.into_inner()).elapsed(),
                idle,
                replaced,
            );
            if quiet || identity_of(socket).ok() != Some(ours) {
                return;
            }
        }
    };
    tokio::select! {
        () = served => {}
        () = watched => {}
    }
    if identity_of(socket).ok() == Some(ours) {
        let _ = std::fs::remove_file(socket);
    }
    Ok(())
}

/// Whether a bridge with `calls` in flight, anyone `bound` in its
/// keyring and its last call `quiet_for` ago ends, its idle `idle`: at
/// once with no one bound, its calls in flight ended with it, so a call
/// held open (a long poll's) never keeps a stopped agent's secret on the
/// wire; once its program is `replaced` and none is in flight, so an
/// upgrade's new build answers the next call; else once idle with none
/// in flight.
fn ends(calls: usize, bound: bool, quiet_for: Duration, idle: Duration, replaced: bool) -> bool {
    !bound || (calls == 0 && (replaced || quiet_for >= idle))
}

/// A file's device and inode: its identity, which another file at its
/// path doesn't share.
fn identity_of(path: &Path) -> std::io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(path)?;
    Ok((meta.dev(), meta.ino()))
}

/// A lock beside a bridge's socket, held while it serves.
struct Lock {
    _held: std::fs::File,
}

impl Lock {
    /// The lock at `path`, if no one holds it.
    fn take(path: &Path) -> anyhow::Result<Option<Self>> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        // SAFETY: flock on a descriptor this owns; no memory is touched.
        let taken = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        Ok(taken.then_some(Self { _held: file }))
    }
}

/// A connection to `bridge`, started first when nothing listens there
/// (and it says how): its socket answered within [`START_WAIT`] of it.
#[expect(clippy::disallowed_methods, reason = "polls a starting bridge")]
pub async fn reach(bridge: &Bridge) -> anyhow::Result<tokio::net::UnixStream> {
    let first = tokio::net::UnixStream::connect(&bridge.socket).await;
    let err = match first {
        Ok(stream) => return Ok(stream),
        Err(err) => err,
    };
    use std::io::ErrorKind::{ConnectionRefused, NotFound};
    if !matches!(err.kind(), NotFound | ConnectionRefused) {
        return Err(err).with_context(|| format!("reaching {}", bridge.socket.display()));
    }
    if bridge.start.is_empty() {
        bail!(
            "nothing serves {}, and nothing starts it: is the module that serves it installed?",
            bridge.socket.display()
        );
    }
    start(bridge)?;
    let until = Instant::now() + START_WAIT;
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        match tokio::net::UnixStream::connect(&bridge.socket).await {
            Ok(stream) => return Ok(stream),
            Err(_) if Instant::now() < until => {}
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "starting the bridge at {}: it didn't listen within {} seconds",
                        bridge.socket.display(),
                        START_WAIT.as_secs()
                    )
                });
            }
        }
    }
}

/// Starts `bridge` apart from whoever starts it: in a systemd user unit
/// of its own where systemd runs Link's services ([`unit`]), else in its
/// own process group, waited for on a thread so it's never left a
/// zombie. Either way nothing is on its standard streams but, in a unit,
/// its log, and its environment is its own: the system's `PATH`, and
/// Link's settings as [`Bridge::start`] hands them.
fn start(bridge: &Bridge) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let Some((program, args)) = bridge.start.split_first() else {
        bail!("nothing starts the bridge at {}", bridge.socket.display());
    };
    let environment = environment(args);
    if let Some(unit) = unit(bridge, &environment) {
        let started = link_secretive::process::command("systemd-run")
            .args(&unit)
            .env_clear()
            .env("PATH", crate::desktop::SYSTEM_PATH)
            .envs(
                ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"]
                    .iter()
                    .filter_map(|name| Some((*name, std::env::var_os(name)?))),
            )
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if started {
            return Ok(());
        }
    }
    let mut command = link_secretive::process::command(program);
    command
        .args(args)
        .env_clear()
        .envs(environment)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {}", program))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// The environment a bridge started with `args` runs in: the system's
/// `PATH`, its settings as `installation_dir` reads them (`LNK_INSTALLATION_DIR`)
/// and the home they're in, and in a debug build a test's own (a CA,
/// names it resolves).
fn environment(args: &[String]) -> Vec<(String, std::ffi::OsString)> {
    let mut environment = vec![("PATH".to_string(), crate::desktop::SYSTEM_PATH.into())];
    if let Some(settings) = args
        .iter()
        .position(|a| a == "--settings")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
    {
        if let Some(home) = settings
            .file_name()
            .filter(|n| *n == ".lnk")
            .and(settings.parent())
        {
            environment.push(("HOME".into(), home.as_os_str().to_owned()));
        }
        environment.push((
            link_files::INSTALLATION_DIR_VAR.into(),
            settings.into_os_string(),
        ));
    }
    if cfg!(debug_assertions) {
        environment.extend(
            link_secretive::process::variables(|name| name.starts_with("LNK_TEST_"))
                .map(|(name, value)| (name, value.into())),
        );
    }
    environment
}

/// `systemd-run`'s arguments that start `bridge` in a user unit of its
/// own, `lnk-bridge-<module>`, its stderr appended to `<module>.log`
/// beside its socket, with `environment`: none on a machine where
/// systemd doesn't run Link's services, or a user manager isn't reached.
fn unit(bridge: &Bridge, environment: &[(String, std::ffi::OsString)]) -> Option<Vec<String>> {
    let reached = cfg!(target_os = "linux")
        && link_service::managed()
        && std::env::var_os("XDG_RUNTIME_DIR").is_some();
    reached.then(|| unit_args(bridge, environment))?
}

/// [`unit`]'s arguments, wherever it runs.
fn unit_args(bridge: &Bridge, environment: &[(String, std::ffi::OsString)]) -> Option<Vec<String>> {
    let module = bridge.socket.file_stem()?.to_str()?;
    let log = bridge.socket.with_extension("log");
    if std::fs::symlink_metadata(&log).is_ok_and(|m| m.len() > MAX_LOG) {
        let _ = std::fs::rename(&log, log.with_extension("log.1"));
    }
    let mut args = vec![
        "--user".to_string(),
        "--collect".into(),
        "--quiet".into(),
        format!("--unit=lnk-bridge-{module}"),
        format!("--property=StandardError=append:{}", log.to_str()?),
    ];
    for (name, value) in environment {
        args.push(format!("--setenv={name}={}", value.to_str()?));
    }
    args.push("--".into());
    args.extend(bridge.start.iter().cloned());
    Some(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        name: "a test's bridge",
        calls: 4,
        per_agent: 4,
        per_owner: 4,
        action_max: 1 << 10,
        action_wait: Duration::from_secs(1),
        answer_wait: Duration::from_secs(1),
    };

    /// With no one bound it ends, a call in flight or not: a call held
    /// open, a long poll's, never keeps a stopped agent's bridge up. With
    /// someone bound, only idle and no call in flight ends it.
    #[test]
    fn no_one_bound_ends_it_whatever_calls_run() {
        let (idle, now, long) = (
            Duration::from_secs(60),
            Duration::ZERO,
            Duration::from_secs(61),
        );
        assert!(ends(0, false, now, idle, false));
        assert!(
            ends(1, false, now, idle, false),
            "a call held open kept it up"
        );
        assert!(!ends(1, true, long, idle, false));
        assert!(!ends(0, true, now, idle, false));
        assert!(ends(0, true, long, idle, false));
    }

    /// A bridge an upgrade replaced ends once no call is in flight, not
    /// at its idle: the next call starts the new build, with its grants.
    #[test]
    fn a_replaced_bridge_ends_once_its_calls_are_done() {
        let (idle, now) = (Duration::from_secs(60), Duration::ZERO);
        assert!(ends(0, true, now, idle, true));
        assert!(!ends(1, true, now, idle, true), "it cut a call off");
    }

    /// The unit is the bridge's own, named for its module, its stderr kept
    /// beside its socket, with the environment it's handed and nothing of
    /// the proxy that starts it.
    #[test]
    fn a_bridge_starts_in_a_unit_of_its_own() {
        let bridge = Bridge {
            socket: PathBuf::from("/run/u/sandbox/bridges/sandbox.sock"),
            start: vec!["/x/lnk-sandbox".into(), "bridge".into()],
        };
        let environment = vec![
            ("PATH".to_string(), "/usr/bin:/bin".into()),
            ("HOME".to_string(), "/home/me".into()),
        ];
        assert_eq!(
            unit_args(&bridge, &environment).unwrap(),
            [
                "--user",
                "--collect",
                "--quiet",
                "--unit=lnk-bridge-sandbox",
                "--property=StandardError=append:/run/u/sandbox/bridges/sandbox.log",
                "--setenv=PATH=/usr/bin:/bin",
                "--setenv=HOME=/home/me",
                "--",
                "/x/lnk-sandbox",
                "bridge",
            ]
        );
    }

    /// A bridge serves past its look while anyone is bound, however long
    /// its idle, and ends at its next look once no one is: nothing of a
    /// stopped agent's lasts until its idle.
    #[tokio::test(start_paused = true)]
    async fn a_bridge_ends_once_no_one_is_bound() {
        let dir = link_test_kit::TempDir::new("brb");
        let keys = dir.join("keys");
        std::fs::create_dir_all(&keys).unwrap();
        std::fs::write(keys.join("agent.json"), "{}").unwrap();
        let started = Started {
            socket: dir.join("b.sock"),
            keys: keys.clone(),
            settings: dir.join("settings"),
            runtime: dir.join("runtime"),
        };
        let serving = serve(
            &started,
            &LIMITS,
            Duration::from_secs(3600),
            |_, _, _| async { Ok(()) },
        );
        tokio::pin!(serving);
        let bound = tokio::time::timeout(LOOK_EVERY * 4, &mut serving).await;
        assert!(bound.is_err(), "it ended while one was bound");
        std::fs::remove_file(keys.join("agent.json")).unwrap();
        tokio::time::timeout(LOOK_EVERY * 2, serving)
            .await
            .expect("it served on with no one bound")
            .unwrap();
    }
}
