//! A check's kit: its folders and ports, the adapter's commands run with
//! its home, the harness launched and healthy, and what was seen.

use super::*;
use link_format::{Note, aside, sentence};

impl Kit {
    pub(super) fn new(check: Check) -> anyhow::Result<Self> {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = TempDir::new(&format!("conformance-kit-{n}"));
        let home = root.join("home");
        let files = root.join("files");
        std::fs::create_dir_all(&home)?;
        std::fs::create_dir_all(&files)?;
        link_fetch::install_crypto_provider();
        let runtime = tokio::runtime::Runtime::new()?;
        let seen = Shared::default();
        let serve = |app: Router| {
            runtime.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
                let port = listener.local_addr()?.port();
                // Each answer goes out at once, so no check waits on Nagle.
                let listener = axum::serve::ListenerExt::tap_io(listener, |tcp| {
                    let _ = tcp.set_nodelay(true);
                });
                tokio::spawn(async move { axum::serve(listener, app).await });
                anyhow::Ok(port)
            })
        };
        let model_port = serve(
            Router::new()
                .route("/v1/chat/completions", post(model))
                .with_state(seen.clone()),
        )?;
        let tool_port = serve(
            Router::new()
                .route("/mcp", post(tool).delete(|| async { StatusCode::OK }))
                .with_state(seen.clone()),
        )?;
        let ports = free_block();
        Ok(Self {
            adapter: check.adapter,
            provider: check.provider,
            stand_in: check.stand_in,
            root,
            home,
            files,
            port: ports.number() as u16,
            _ports: ports,
            model_port,
            tool_port,
            seen,
            runtime,
        })
    }

    /// The environment `lnk-agent` gives an adapter's programs.
    fn env(&self) -> Vec<(String, String)> {
        vec![
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
            ("HOME".into(), self.home.display().to_string()),
            ("LANG".into(), "en_US.UTF-8".into()),
            ("LNK_FILES".into(), self.files.display().to_string()),
            ("LNK_AGENT".into(), AGENT.into()),
            ("LNK_AGENT_ID".into(), AGENT_ID.into()),
            ("LNK_OWNER".into(), "conformance@box".into()),
            (harness::PORT_ENV.into(), self.port.to_string()),
            // As Link's sandbox gives a harness each tool it may call.
            (
                format!("LNK_TOOL_{}", TOOL.to_ascii_uppercase()),
                self.tool_url(),
            ),
        ]
    }

    /// The kit's tool's address, as Link's bridge gives one.
    fn tool_url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.tool_port)
    }

    /// What the harness has logged.
    pub(super) fn log(&self) -> String {
        let log = std::fs::read_to_string(self.root.join("harness.log")).unwrap_or_default();
        format!("the harness's log:\n{log}")
    }

    /// `adapter <command>`, with `input` on stdin: what it printed, or
    /// an error saying how it failed.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    pub(super) fn run(&self, command: &str, input: Option<&str>) -> anyhow::Result<String> {
        let mut child = Command::new(&self.adapter)
            .arg(command)
            .env_clear()
            .envs(self.env())
            .envs(match command {
                "install" | "configure" => network_env(),
                _ => Vec::new(),
            })
            .current_dir(&self.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("running {}", self.adapter.display()))?;
        if let Some(input) = input {
            let written = child
                .stdin
                .take()
                .context("no stdin")?
                .write_all(input.as_bytes());
            // A command that reads none of it (`needs`) may have ended.
            match written {
                Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => {}
                other => other?,
            }
        }
        drop(child.stdin.take());
        let output = child.wait_with_output()?;
        ensure!(
            output.status.success(),
            "{command} failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub(super) fn contract(&self) -> anyhow::Result<Contract> {
        let out = self.run("contract", None)?;
        let contract: Contract = serde_json::from_str(&out)
            .with_context(|| format!("contract didn't print a Contract: {out}"))?;
        ensure!(
            contract.version == harness::PROTOCOL.current,
            "contract speaks version {}, not this Link's, {}",
            contract.version,
            harness::PROTOCOL.current
        );
        Ok(contract)
    }

    /// The settings `configure` gets: a local model, the endpoint's key,
    /// the kit's tool, no connection. `keys: false`, as `needs` gets them:
    /// no key.
    pub(super) fn settings(&self, keys: bool) -> String {
        let settings = Settings {
            files: self.files.clone(),
            model: Model {
                provider: self.provider.clone(),
                name: Some(MODEL.into()),
                base_url: Some(format!("http://127.0.0.1:{}", self.model_port)),
                ..Model::default()
            },
            endpoint_key: keys.then(|| KEY.to_string().into()),
            tools: [(TOOL.to_string(), self.tool_url())].into(),
            ..Settings::default()
        };
        serde_json::to_string(&settings).unwrap_or_default()
    }

    /// Its harness's stand-in, and the files it's said with, in its home.
    pub(super) fn stand_in(&self) -> anyhow::Result<()> {
        let Some(stand_in) = &self.stand_in else {
            return Ok(());
        };
        for (path, text) in &stand_in.files {
            let path = self.home.join(path);
            std::fs::create_dir_all(path.parent().context("no folder")?)?;
            std::fs::write(&path, text)?;
        }
        let fixture = link_test_kit::programs_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
            .join("lnk-harness-fixture");
        ensure!(
            fixture.is_file(),
            "{}",
            link_test_kit::programs::not_built(&fixture, link_test_kit::programs::BUILD)
        );
        let settings: Settings = serde_json::from_str(&self.settings(false))?;
        let model = serde_json::to_string(&settings.model)?;
        let tools = match stand_in.tools_in_settings {
            true => " LNK_HARNESS_FIXTURE_TOOLS=env",
            false => "",
        };
        let state = if stand_in.native_state {
            ""
        } else {
            " LNK_HARNESS_FIXTURE_STATE=none"
        };
        let script = format!(
            "#!/bin/sh\n{prelude}\n\
             for arg in \"$0\" \"$@\"; do\n\
             \x20 case \"$arg\" in acp|*-acp) LNK_HARNESS_FIXTURE_MODEL='{model}'{tools}{state} exec '{}' acp;; esac\n\
             done\n\
             exit 0\n",
            fixture.display(),
            prelude = stand_in.prelude,
        );
        for name in std::iter::once(&stand_in.program).chain(&stand_in.entrypoints) {
            let program = self.home.join(name);
            std::fs::create_dir_all(program.parent().context("no folder")?)?;
            link_test_kit::write_program(&program, &script)?;
        }
        Ok(())
    }

    pub(super) fn configure(&self) -> anyhow::Result<()> {
        self.run("configure", Some(&self.settings(true)))?;
        Ok(())
    }

    /// The harness, in the foreground, in a process group of its own.
    pub(super) fn launch(&self, launch: &Launch) -> anyhow::Result<Running> {
        let (program, args) = launch
            .argv
            .split_first()
            .context("command printed no argv")?;
        let log = std::fs::File::create(self.root.join("harness.log"))?;
        let mut command = link_test_kit::env::command(program, &self.home);
        command
            .args(args)
            .env_clear()
            .envs(self.env())
            .envs(&launch.env)
            .current_dir(&self.home)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        let child = link_test_kit::ChildGroup::spawn(&mut command)
            .with_context(|| format!("running {program}"))?;
        Ok(Running {
            child,
            closed: false,
        })
    }

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    pub(super) fn wait_healthy(&self, launch: &Launch) -> anyhow::Result<()> {
        let started = Instant::now();
        while self.run("health", None).is_err() {
            if started.elapsed() > launch.wait() {
                bail!(
                    "health failed for {:?} after it started; {}",
                    launch.wait(),
                    self.log()
                );
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Ok(())
    }

    /// Its endpoint, when it has one (an adapter that speaks every
    /// connection itself has none).
    pub(super) fn endpoint(&self) -> anyhow::Result<Option<String>> {
        let Ok(out) = self.run("endpoint", None) else {
            return Ok(None);
        };
        let url: String = serde_json::from_str(&out)
            .with_context(|| format!("endpoint didn't print a string: {out}"))?;
        let port = harness::endpoint_port(&url)?;
        ensure!(
            (self.port..self.port + harness::PORT_BLOCK).contains(&port),
            "its endpoint is on port {port}, outside the agent's block from {} (LNK_PORT)",
            self.port
        );
        Ok(Some(url))
    }

    /// That its endpoint, answering the proof, sets `TCP_NODELAY` on the
    /// connection it accepted, as the contract asks: read from the socket
    /// itself, held by a process of the harness's group `group`. Where
    /// this system can't say, it's said and passed over.
    pub(super) fn nodelay_checks(&self, url: &str, group: u32) -> anyhow::Result<()> {
        use std::io::Read;
        let authority = url
            .strip_prefix("http://")
            .and_then(|rest| rest.split('/').next())
            .with_context(|| format!("its endpoint isn't at an http address: {url}"))?;
        let mut stream = std::net::TcpStream::connect(authority)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        write!(
            stream,
            "GET {} HTTP/1.1\r\nHost: {authority}\r\n{}: {}\r\n{}: {AGENT}\r\n\r\n",
            harness::PROOF_PATH,
            harness::CHALLENGE_HEADER,
            "0123456789abcdef".repeat(4),
            link_connection_contract::AGENT_HEADER
        )?;
        // Its answer's head: the connection accepted and answered, and
        // kept open while the flag is read.
        let mut head = Vec::new();
        let mut read = [0; 1024];
        while !head.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut read)?;
            ensure!(
                n > 0,
                "its endpoint closed the proof's connection unanswered"
            );
            head.extend_from_slice(&read[..n]);
        }
        let (ours, theirs) = (stream.local_addr()?.port(), stream.peer_addr()?.port());
        match link_test_kit::nodelay_of(group, theirs, ours) {
            Some(nodelay) => ensure!(
                nodelay,
                "its endpoint answers the proof without TCP_NODELAY on the connection it \
                 accepted: each message would wait about 40 ms for a delayed ACK"
            ),
            None => aside(Note(sentence!(
                "Whether its endpoint sets TCP_NODELAY wasn't checked: this system doesn't say."
            ))),
        }
        Ok(())
    }
}

impl Kit {
    /// What the kit's model and tool have been sent.
    pub(super) fn seen(&self) -> std::sync::MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Waits, at most a minute, until `done` holds of what's been seen.
    pub(super) async fn until(
        &self,
        what: &str,
        done: impl Fn(&Seen) -> bool,
    ) -> anyhow::Result<()> {
        link_test_kit::within_async(what, Duration::from_secs(60), || done(&self.seen())).await?;
        Ok(())
    }

    /// The last request's messages, as JSON.
    pub(super) fn last_asked(&self) -> String {
        self.seen()
            .asked
            .last()
            .map(|a| a["messages"].to_string())
            .unwrap_or_default()
    }
}
