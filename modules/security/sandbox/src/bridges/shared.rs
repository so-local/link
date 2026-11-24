//! A shared tool (`lnk sandbox tool add --shared`): one server for every
//! session of every sandbox that names it, where any other tool runs a
//! server for each. A team's tools are shared, so a dozen agents on a
//! team run one.
//!
//! The server runs under its hub (`lnk sandbox mcp-shared <name>`), a
//! process of its own that the first proxy to need it starts, outside
//! every sandbox. The hub listens on a Unix socket in the runtime folder
//! (`sandbox/tools/<key>.sock`, the key of the tool as it's named and
//! defined, so a tool defined again gets a hub of its own), which only
//! the user and no sandbox reaches, and each proxy session is a
//! connection to it, speaking MCP over stdio's lines. The hub starts the server once, initializes it
//! once, and passes each session's messages on with ids of its own, so
//! answers go back to the session that asked. A lock beside the socket
//! keeps it to one hub. It stops, and its server with it, once no
//! session has been open for the sandbox's `shared_idle`, and when its
//! server ends: the next session starts both again.
//!
//! What a call may do is still each proxy's: its tool's rules are asked
//! there, before a call reaches the hub.

mod hub;

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use link_format::WithinExt;

use crate::bridges::tools::Tool;
pub use hub::serve;

/// The longest a socket's path may be (macOS's `sun_path`, the shorter).
const SOCKET_MAX: usize = 103;

/// How long a proxy waits for a hub it started to listen.
const START_TIMEOUT: Duration = Duration::from_secs(30);

/// The most of its server's output a hub keeps in its log while it runs;
/// a log past it when a hub starts is kept as `<name>.log.1`, the one
/// before that let go.
const MAX_LOG: u64 = 1024 * 1024;

/// Where `tool`'s hub listens. Its settings in `dir` determine its name,
/// but its socket stays in the private runtime folder. Named by its key
/// alone, which signs the tool's name: macOS's runtime folder
/// (`/var/folders/...`) leaves a socket's name about 40 bytes of
/// [`SOCKET_MAX`].
pub fn socket(runtime: &Path, dir: &Path, tool: &Tool) -> anyhow::Result<PathBuf> {
    Ok(link_files::layout::Runtime::SharedTools
        .at(runtime)
        .join(format!("{}.sock", key(dir, tool)?)))
}

/// What `tool` is named and its server runs as, its command, folder and
/// environment, as 16 hex digits: the same for the proxy and the hub, one program.
/// Signed with this machine's own secret ([`machine_secret`]), never a
/// hash anyone can make: its environment holds tokens (`--env`), and a
/// socket's name is seen by every user of the machine, so an unkeyed
/// one would let a weak token be guessed from it, offline.
pub fn key(dir: &Path, tool: &Tool) -> anyhow::Result<String> {
    let secret = machine_secret(dir)?;
    // Each part with its length before it, so no two tools read alike.
    let mut message = link_secretive::Secret::new(Vec::<u8>::new());
    let bytes = message.expose_mut();
    let mut part = |part: &[u8]| {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part);
    };
    part(tool.name.as_bytes());
    for arg in &tool.argv {
        part(arg.as_bytes());
    }
    for (name, value) in &tool.env {
        part(name.as_bytes());
        part(value.expose().as_bytes());
    }
    let tag = secret.sign(message.expose());
    Ok(tag[..8].iter().map(|b| format!("{b:02x}")).collect())
}

/// The secret [`key`] signs with, this machine's alone: made once, kept
/// in `dir`'s `tools/.key` (mode 600). Two proxies making it at once
/// keep the first one's: each links its own in, and only one link
/// lands.
fn machine_secret(dir: &Path) -> anyhow::Result<link_secretive::OpaqueSecret> {
    let file = dir.join("tools").join(".key");
    let read = |file: &Path| -> anyhow::Result<Option<link_secretive::OpaqueSecret>> {
        match std::fs::File::open(file) {
            Ok(mut opened) => {
                let kept = link_secretive::Secret::read_line(&mut BufReader::new(&mut opened))
                    .within(format!("reading {}", file.display()))?
                    .filter(|kept| kept.expose().len() == 64)
                    .within(format!("{} isn't one Link made", file.display()))?;
                Ok(Some(link_secretive::OpaqueSecret::from_text(kept)))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).within(format!("reading {}", file.display())),
        }
    };
    if let Some(secret) = read(&file)? {
        return Ok(secret);
    }
    let made = link_secretive::OpaqueSecret::random(32)?;
    let kept = made.keep().within("the tools' secret kept as text")?;
    let mine = file.with_file_name(format!("key.{}", std::process::id()));
    link_files::write_private(&mine, kept.expose().as_bytes())
        .within(format!("writing {}", mine.display()))?;
    let linked = std::fs::hard_link(&mine, &file);
    let _ = std::fs::remove_file(&mine);
    match linked {
        Ok(()) => Ok(made),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            read(&file)?.within(format!("{} went missing", file.display()))
        }
        Err(err) => Err(err).within(format!("keeping {}", file.display())),
    }
}

/// A connection to `tool`'s hub, started first if none answers: this
/// program, `sandbox mcp-shared <name>`, apart from the proxy that
/// started it, so it outlives it and isn't any agent's: in a unit of its
/// own where systemd runs Link's services (else stopping the agent that
/// started it would stop every agent's), else in a process group of its
/// own.
#[expect(
    clippy::disallowed_methods,
    reason = "retries while the shared bridge starts"
)]
pub async fn connect(
    runtime: &Path,
    dir: &Path,
    tool: &Tool,
) -> anyhow::Result<tokio::net::UnixStream> {
    let socket = socket(runtime, dir, tool)?;
    link_files::create_private_dir(socket.parent().expect("a shared tool has a runtime folder"))?;
    let key = key(dir, tool)?;
    anyhow::ensure!(
        socket.as_os_str().len() <= SOCKET_MAX,
        "tool {}'s socket, {}, is too long a path for one",
        tool.name,
        socket.display()
    );
    if let Ok(stream) = tokio::net::UnixStream::connect(&socket).await {
        return Ok(stream);
    }
    let exe = std::env::current_exe().within("finding lnk-sandbox")?;
    // Its own log, beside its socket: it outlives the proxy whose output
    // it would otherwise hold.
    let log_path = socket.with_extension("log");
    rotate(&log_path, MAX_LOG);
    let args: Vec<std::ffi::OsString> = ["mcp-shared", &tool.name, &key, "--dir"]
        .into_iter()
        .map(Into::into)
        .chain([
            dir.as_os_str().to_owned(),
            "--runtime".into(),
            runtime.as_os_str().to_owned(),
        ])
        .collect();
    if !in_unit(&exe, &args, &tool.name, &key, &log_path) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .within(format!("opening tool {}'s log", tool.name))?;
        let mut hub = link_secretive::process::ours(exe);
        hub.args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(log);
        std::os::unix::process::CommandExt::process_group(&mut hub, 0);
        let mut hub = hub
            .spawn()
            .within(format!("starting tool {}'s hub", tool.name))?;
        // Waited for when it stops, so it's never left a zombie of the proxy.
        std::thread::spawn(move || hub.wait());
    }
    let started = Instant::now();
    loop {
        match tokio::net::UnixStream::connect(&socket).await {
            Ok(stream) => return Ok(stream),
            Err(_) if started.elapsed() < START_TIMEOUT => {
                tokio::time::sleep(Duration::from_millis(50)).await
            }
            Err(err) => {
                return Err(err).within(format!("tool {}'s server didn't start", tool.name));
            }
        }
    }
}

/// Starts the hub, `exe` with `args`, in a systemd user unit of its own
/// (`lnk-tool-<name>-<key>`), where systemd runs Link's services on
/// Linux: whether it did.
fn in_unit(exe: &Path, args: &[std::ffi::OsString], name: &str, key: &str, log: &Path) -> bool {
    if !cfg!(target_os = "linux")
        || !link_service::managed()
        || std::env::var_os("XDG_RUNTIME_DIR").is_none()
    {
        return false;
    }
    let mut run = link_secretive::process::command("systemd-run");
    run.args(["--user", "--collect", "--quiet"])
        .arg(format!("--unit=lnk-tool-{name}-{key}"))
        .arg(format!("--property=StandardError=append:{}", log.display()));
    run.args(link_service::setenv_args());
    run.arg("--")
        .arg(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Keeps `log` as `<log>.1` when it's past `max`, letting the one kept
/// before go.
fn rotate(log: &Path, max: u64) {
    if std::fs::symlink_metadata(log).is_ok_and(|m| m.len() > max) {
        let mut kept = log.as_os_str().to_owned();
        kept.push(".1");
        let _ = std::fs::rename(log, kept);
    }
}

/// `dir`, handed to a hub as Link's `sandbox/` settings folder, if it's
/// one: a folder, not a link, only this user's (`700`), as Link makes
/// it. Anything else is refused: the hub runs outside every sandbox.
pub fn check_dir(dir: &Path) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = std::fs::symlink_metadata(dir).within(format!("reading {}", dir.display()))?;
    // SAFETY: geteuid can't fail.
    let me = unsafe { libc::geteuid() };
    anyhow::ensure!(
        dir.is_absolute()
            && meta.is_dir()
            && meta.uid() == me
            && meta.permissions().mode() & 0o077 == 0,
        "{} isn't Link's settings folder for the sandbox",
        dir.display()
    );
    Ok(dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A log past the most is kept once, beside it, when a hub starts.
    #[test]
    fn a_long_log_is_kept_once() {
        let dir = std::env::temp_dir().join(format!("lnk-hub-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("t.log");
        std::fs::write(&log, "12345").unwrap();
        rotate(&log, 10);
        assert!(log.exists(), "short: kept as is");
        std::fs::write(&log, "0123456789ab").unwrap();
        rotate(&log, 10);
        assert!(!log.exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("t.log.1")).unwrap(),
            "0123456789ab"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A socket's name, seen by every user of the machine, is signed with
    /// this machine's secret: no one without it can test a guess of a
    /// tool's token against it, and another machine names the same tool
    /// otherwise. The secret is made once, only its user's, and kept.
    #[test]
    fn a_sockets_name_is_signed_with_the_machines_secret() {
        use link_secretive::Duplicate;
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("lnk-hub-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (one, two) = (root.join("one"), root.join("two"));
        let mut tool = Tool {
            name: "gh".into(),
            argv: vec!["gh-mcp".into()],
            env: [(
                "GH_TOKEN".to_string(),
                link_secretive::Secret::new("1234".to_string()),
            )]
            .into(),
            ask: crate::bridges::tools::Ask::All,
            allow: vec![],
            ask_if: Vec::new(),
            ask_if_hint: None,
            shared: true,
        };
        let first = key(&one, &tool).unwrap();
        assert_eq!(first.len(), 16);
        assert_eq!(
            key(&one, &tool).unwrap(),
            first,
            "the same tool, the same name"
        );
        let kept = one.join("tools").join(".key");
        let mode = std::fs::metadata(&kept).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "the machine's secret is only its user's"
        );
        assert_eq!(std::fs::read_to_string(&kept).unwrap().trim().len(), 64);
        assert_ne!(
            key(&two, &tool).unwrap(),
            first,
            "another machine's secret, another name"
        );

        // The name is no unkeyed hash of what the tool runs as: not the
        // one this program made before, that anyone could make again.
        let unkeyed = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            tool.argv.hash(&mut hasher);
            for (name, value) in &tool.env {
                (name, value.expose()).hash(&mut hasher);
            }
            format!("{:016x}", hasher.finish())
        };
        assert_ne!(first, unkeyed);

        // Another name is another tool, though it runs the same.
        let renamed = Tool {
            name: "gh-work".into(),
            ..tool.duplicate()
        };
        assert_ne!(key(&one, &renamed).unwrap(), first);
        // Its socket fits macOS's runtime folder, whatever the tool's name.
        let mac = Path::new("/var/folders/jk/97dv7hyd3q59zhf1lb3t9tx40000gn/T/lnk-runtime-501");
        let long = Tool {
            name: "browser-spgjgu2icnyb".into(),
            ..tool.duplicate()
        };
        let at = socket(mac, &one, &long).unwrap();
        assert!(at.as_os_str().len() <= SOCKET_MAX, "{}", at.display());

        // A token changed with `--env` is a tool defined again.
        tool.env.insert(
            "GH_TOKEN".into(),
            link_secretive::Secret::new("1235".to_string()),
        );
        assert_ne!(key(&one, &tool).unwrap(), first);
        let runtime = root.join("runtime");
        assert!(
            socket(&runtime, &one, &tool)
                .unwrap()
                .starts_with(link_files::layout::Runtime::SharedTools.at(&runtime))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A Mac with no system runtime folder: its `$TMPDIR` alone is about
    /// 50 bytes, and a runtime folder under it left a shared tool's socket
    /// too long. The runtime folder is never under it, and a socket is
    /// named by its tool's key, not its name, so the longest name a tool
    /// may have, for the largest user ID, fits.
    #[test]
    fn the_longest_tools_socket_fits_on_a_mac() {
        let mac_tmp = Path::new("/var/folders/jk/97dv7hyd3q59zhf1lb3t9tx40000gn/T/");
        let longest = "t".repeat(40);
        link_sandbox_client::routes::check_tool_name(&longest).unwrap();
        assert!(
            link_sandbox_client::routes::check_tool_name(&format!("{longest}t")).is_err(),
            "40 is the longest a tool's name may be"
        );
        let settings = link_test_kit::TempDir::new("tool-socket");
        let tool = Tool {
            name: longest,
            argv: vec!["server".into()],
            env: Default::default(),
            ask: crate::bridges::tools::Ask::All,
            allow: vec![],
            ask_if: Vec::new(),
            ask_if_hint: None,
            shared: true,
        };
        let runtime = link_files::layout::LAYOUT
            .runtime_root
            .fallback_for(u32::MAX);
        assert!(!runtime.starts_with(mac_tmp));
        let socket = socket(&runtime, &settings, &tool).unwrap();
        assert!(
            socket.as_os_str().len() <= SOCKET_MAX,
            "{} is {} bytes, past {SOCKET_MAX}",
            socket.display(),
            socket.as_os_str().len()
        );
    }
}
