//! The embedding model an engine asks for, served for it outside its
//! sandbox ([`serve`]), and kept at one address until it comes up.

use super::*;
use link_format::WithinExt;

/// The argument [`serve`] gives the model's address: `LNK_MEMORY_MODEL=`.
pub(super) fn model_placeholder() -> String {
    format!("{MODEL_ENV}=")
}

/// The argument [`serve`] gives the model's port, after the sandbox's
/// [`super::engine::MODELS_FLAG`]: the two dropped when it has none.
pub(super) fn port_placeholder() -> String {
    format!("{MODEL_ENV}:port")
}

/// The port of a model's address on this machine's loopback
/// (`http://127.0.0.1:<port>`, as `lnk model serve` says it listens):
/// what a sandbox reaches it at. None for an empty address, or one
/// elsewhere.
pub(super) fn port_of(url: &str) -> Option<u16> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next()?;
    let (host, port) = authority.rsplit_once(':')?;
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
        .then(|| port.parse().ok())
        .flatten()
}

/// `argv` with the model's address `url` and its port filled in
/// ([`model_placeholder`], [`port_placeholder`]); the port's flag
/// dropped, with its placeholder, when the address has none.
fn filled(argv: &[String], url: &str) -> Vec<String> {
    let (address, port) = (model_placeholder(), port_placeholder());
    let mut filled = Vec::with_capacity(argv.len());
    let mut args = argv.iter().peekable();
    while let Some(arg) = args.next() {
        if *arg == super::engine::MODELS_FLAG && args.peek() == Some(&&port) {
            args.next();
            if let Some(at) = port_of(url) {
                filled.extend([arg.clone(), at.to_string()]);
            }
        } else if *arg == address {
            filled.push(format!("{address}{url}"));
        } else {
            filled.push(arg.clone());
        }
    }
    filled
}

/// How long `lnk model serve` may take to say where it listens, before
/// the engine starts without it (searching by words): well within the
/// time a harness gives a tool to start (Link Harness gives 30 s), so a
/// runtime that doesn't come up doesn't take memory away.
pub(super) const MODEL_START: Duration = Duration::from_secs(10);

/// `lnk agent memory serve`, the start of an agent's memory tool, outside
/// its sandbox: `model` served (`lnk model serve --model <model> --json`), then
/// the engine's command `argv`, its argument `LNK_MEMORY_MODEL=` given the
/// model's address, or left empty if it couldn't be served, and its
/// sandbox given the model's port alone ([`filled`]). The model
/// is stopped when the engine ends ([`Kept::stop`]). On Linux the engine
/// gets a SIGTERM if this process ends first.
pub fn serve(model: &str, argv: &[String]) -> anyhow::Result<()> {
    let server = Served::model(model);
    let url = server
        .as_ref()
        .and_then(|s| s.ready(std::time::Instant::now() + MODEL_START))
        .unwrap_or_default();
    let argv = filled(argv, &url);
    let mut engine = link_secretive::process::command(&argv[0]);
    engine.args(&argv[1..]);
    // Its session can end with this process killed rather than its stdin
    // closed (the sandbox kills its tool's process): the engine is told
    // to stop then too, whether or not it reads its input.
    link_secretive::process::ends_with_parent(&mut engine, libc::SIGTERM);
    let status = engine.status().within(format!("running {}", argv[0]));
    drop(server);
    let status = status?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// When a model's server that ended is started again, while memory is
/// served: a runtime that comes up late, or comes back, is found.
const RESTART: Restart = Restart {
    after: Duration::from_secs(30),
    at_most: Duration::from_secs(30),
};

/// `lnk model serve` says no runtime here has the model
/// (`link-model`' own code).
const NO_SUCH_MODEL: i32 = 3;

/// A model served for memory outside its sandbox, at one address for as
/// long as it's kept: `lnk model serve --model <model> --json --port <port>`,
/// kept running ([`Kept`]) so the engine, given the address once, finds
/// the model whenever it's there. Stopped when dropped.
pub(super) struct Served {
    url: String,
    /// What its first start came to, once: where it listens, or that no
    /// runtime has the model (`None`).
    first: std::sync::mpsc::Receiver<Option<String>>,
    kept: Kept,
}

/// What a served model's first start comes to, told once: where it
/// says it listens, or, as it ends, that no runtime has the model (and
/// it's given up on) or that it failed (its address is the engine's
/// anyway).
struct FirstStart(Option<std::sync::mpsc::Sender<Option<String>>>);

impl Watch for FirstStart {
    fn started(&mut self, child: &mut Child) {
        let (Some(said), Some(stdout)) = (&self.0, child.stdout.take()) else {
            return;
        };
        let said = said.clone();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::BufReader::new(stdout).read_line(&mut line);
            let url = serde_json::from_str::<serde_json::Value>(&line)
                .ok()
                .and_then(|v| v.get("url")?.as_str().map(String::from));
            if let Some(url) = url {
                let _ = said.send(Some(url));
            }
        });
    }

    fn ended(&mut self, how: &std::io::Result<std::process::ExitStatus>) -> bool {
        let Some(said) = self.0.take() else {
            return true;
        };
        if how.as_ref().ok().and_then(|s| s.code()) == Some(NO_SUCH_MODEL) {
            let _ = said.send(None);
            return false;
        }
        let _ = said.send(Some(String::new()));
        true
    }
}

impl Served {
    /// `model` served, by `lnk model serve`.
    pub(super) fn model(model: &str) -> Option<Self> {
        let model = model.to_string();
        Self::start(move |port| {
            let mut serve = link_core::lnk();
            serve.args([
                "model",
                "serve",
                &model,
                "--json",
                "--port",
                &port.to_string(),
            ]);
            serve
        })
    }

    /// Served by the command `serve` makes for a port, free now on this
    /// machine's loopback; its first start's output read for where it
    /// listens.
    pub(super) fn start(serve: impl Fn(u16) -> Command + Send + 'static) -> Option<Self> {
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .ok()?
            .local_addr()
            .ok()?
            .port();
        let (said, first) = std::sync::mpsc::channel();
        let mut starts = 0;
        let kept = Kept::start(
            move || {
                let mut command = serve(port);
                command.stderr(Stdio::null());
                command.stdout(match starts {
                    0 => Stdio::piped(),
                    _ => Stdio::null(),
                });
                starts += 1;
                command
            },
            RESTART,
            FirstStart(Some(said)),
        );
        Some(Self {
            url: format!("http://127.0.0.1:{port}"),
            first,
            kept,
        })
    }

    /// The address it was given, where it's started until it comes up:
    /// for an engine that doesn't wait for it.
    pub(super) fn url(&self) -> String {
        self.url.clone()
    }

    /// Where it listens, as its first start says, or by `until` the
    /// address it was given anyway: it's started again there until it
    /// comes up. None when no runtime here has the model.
    pub(super) fn ready(&self, until: std::time::Instant) -> Option<String> {
        let wait = until.saturating_duration_since(std::time::Instant::now());
        match self.first.recv_timeout(wait) {
            Ok(None) => None,
            Ok(Some(said)) if !said.is_empty() => Some(said),
            Ok(Some(_)) | Err(_) => Some(self.url.clone()),
        }
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.kept.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model's port on this machine is what its sandbox reaches; one
    /// elsewhere, or none, gives it no port.
    #[test]
    fn a_models_port_is_read_from_its_loopback_address() {
        assert_eq!(port_of("http://127.0.0.1:41234"), Some(41234));
        assert_eq!(port_of("http://localhost:11434/"), Some(11434));
        assert_eq!(port_of("http://[::1]:8080/v1"), Some(8080));
        for none in [
            "",
            "http://192.168.1.5:11434",
            "https://127.0.0.1:1",
            "http://127.0.0.1",
        ] {
            assert_eq!(port_of(none), None, "{none}");
        }
    }

    /// The engine's address and its sandbox's port are filled in from
    /// where the model listens; with no model there, the port's flag goes
    /// and the address is empty.
    #[test]
    fn the_served_models_address_and_port_are_filled_in() {
        let argv: Vec<String> = [
            "sandbox",
            "run",
            super::super::engine::MODELS_FLAG,
            &port_placeholder(),
            "--network.macos.mode",
            "host",
            "--",
            "/usr/bin/env",
            &model_placeholder(),
            "lnk-memory-tool",
        ]
        .map(String::from)
        .to_vec();
        let served = filled(&argv, "http://127.0.0.1:41234");
        assert_eq!(served[2..4], [super::super::engine::MODELS_FLAG, "41234"]);
        assert_eq!(served[8], "LNK_MEMORY_MODEL=http://127.0.0.1:41234");
        let missing = filled(&argv, "");
        assert_eq!(missing.len(), argv.len() - 2);
        assert!(
            !missing
                .iter()
                .any(|a| a == super::super::engine::MODELS_FLAG)
        );
        assert!(missing.contains(&"LNK_MEMORY_MODEL=".to_string()));
    }

    /// A model's server is waited for no longer than it takes to say
    /// where it listens, and well before a harness gives up on the tool;
    /// slow, or failed, its address is the engine's anyway, and it's
    /// started again there until it comes up; one that says no runtime
    /// has the model is given up on.
    #[cfg(unix)]
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_model_server_is_kept_at_one_address_until_it_comes_up() {
        assert!(MODEL_START * 2 < Duration::from_secs(30));
        let dir = std::env::temp_dir().join(format!("lnk-memory-served-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let soon = || std::time::Instant::now() + Duration::from_secs(10);
        // Says where it listens: used as soon as it does, where it says.
        let elsewhere = Served::start(|_| {
            let mut says = Command::new("sh");
            says.args(["-c", r#"echo '{"url":"http://127.0.0.1:9"}'; cat"#]);
            says
        })
        .unwrap();
        assert_eq!(
            elsewhere.ready(soon()).as_deref(),
            Some("http://127.0.0.1:9")
        );
        drop(elsewhere);
        let says = Served::start(|port| {
            let mut says = Command::new("sh");
            let line = format!(r#"echo '{{"url":"http://127.0.0.1:{port}"}}'; cat"#);
            says.args(["-c", &line]);
            says
        })
        .unwrap();
        let started = std::time::Instant::now();
        assert_eq!(says.ready(soon()), Some(says.url.clone()));
        assert!(started.elapsed() < Duration::from_secs(5));
        drop(says);
        // Fails at first (a runtime not up yet): its address anyway, at
        // once, and it's started again at the same one.
        let log = dir.join("starts");
        let fails = {
            let log = log.clone();
            Served::start(move |port| {
                let mut fails = Command::new("sh");
                fails.args(["-c", &format!("echo {port} >> '{}'; exit 1", log.display())]);
                fails
            })
            .unwrap()
        };
        let started = std::time::Instant::now();
        let url = fails.ready(soon());
        assert_eq!(url, Some(fails.url.clone()));
        assert!(started.elapsed() < Duration::from_secs(5));
        let port = fails.url.rsplit(':').next().unwrap().to_string();
        assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), port);
        drop(fails);
        // No runtime has the model: given up on, as it says, at once, so
        // a session waits on none of it.
        let started = std::time::Instant::now();
        let missing = Served::start(|_| {
            let mut missing = Command::new("sh");
            missing.args(["-c", &format!("exit {NO_SUCH_MODEL}")]);
            missing
        })
        .unwrap();
        assert_eq!(missing.ready(soon()), None);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "{:?}",
            started.elapsed()
        );
        // Stopped when dropped: its stdin closed, it ends.
        let hangs = Served::start(|_| {
            let mut hangs = Command::new("sh");
            hangs.args(["-c", "cat"]);
            hangs
        })
        .unwrap();
        let until = std::time::Instant::now() + Duration::from_millis(300);
        assert_eq!(hangs.ready(until), Some(hangs.url.clone()));
        let child = hangs.kept.pid().unwrap();
        drop(hangs);
        if cfg!(target_os = "linux") {
            assert!(!Path::new(&format!("/proc/{child}")).exists());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
