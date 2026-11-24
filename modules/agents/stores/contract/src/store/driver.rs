//! A driver run here, as its caller runs it: its program, folder and
//! environment ([`Driver`]).

use super::*;

/// A driver, as its caller runs it: its program, the folder it keeps
/// what it keeps in, and what its caller sets in its environment besides
/// (its settings' file, as its own docs name it).
#[derive(Clone, Debug, PartialEq)]
pub struct Driver {
    pub name: String,
    pub program: PathBuf,
    pub dir: PathBuf,
    pub env: Vec<(String, String)>,
}

/// The places whose saved names aren't their drivers' module IDs, and
/// those IDs: a spec names the place, `lnk module add` the driver.
pub const MODULES: [(&str, &str); 2] = [
    (crate::storage::FILE, "file-store"),
    (crate::storage::SQLITE, "sqlite-store"),
];

/// The module ID of place `name`'s driver.
pub fn module_for(name: &str) -> &str {
    MODULES
        .iter()
        .find(|(place, _)| *place == name)
        .map_or(name, |(_, module)| module)
}

impl Driver {
    /// Driver `name`, its program `lnk-<name>` next to this program or on
    /// the PATH, keeping what it keeps in `dir`.
    pub fn find(name: &str, dir: &Path) -> anyhow::Result<Self> {
        check_name(name)?;
        let module = module_for(name);
        let Some(program) = crate::find_module(module) else {
            bail!(
                "no store driver {name} (lnk-{module}) next to this program or on the PATH: lnk \
                 module add {module}"
            );
        };
        Ok(Self::at(name, program, dir))
    }

    /// Driver `name` at `program`.
    pub fn at(name: &str, program: PathBuf, dir: &Path) -> Self {
        Self {
            name: name.to_string(),
            program,
            dir: dir.to_path_buf(),
            env: Vec::new(),
        }
    }

    /// This one, run with `key` set to `value` besides.
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_string(), value.to_string()));
        self
    }

    /// What `lnk-<name> contract` says: an error unless it's this
    /// version's.
    pub fn contract(&self) -> anyhow::Result<Contract> {
        let out = link_secretive::process::ours(&self.program)
            .arg("contract")
            .env(DIR_ENV, &self.dir)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .with_context(|| format!("running lnk-{} contract", module_for(&self.name)))?;
        anyhow::ensure!(
            out.status.success(),
            "lnk-{} contract failed ({})",
            module_for(&self.name),
            out.status
        );
        let contract: Contract = serde_json::from_slice(&out.stdout)
            .with_context(|| format!("reading lnk-{}'s contract", module_for(&self.name)))?;
        anyhow::ensure!(
            contract.version == VERSION,
            "lnk-{} speaks the store's contract version {}, not {VERSION}",
            module_for(&self.name),
            contract.version
        );
        Ok(contract)
    }

    /// `request`, answered by this driver: by the one process this
    /// program kept running for its folder and environment
    /// (`lnk-<name> serve`) when its contract says `serve`, started again
    /// once it stops, else by one run for it ([`Driver::call_once`]).
    /// Either is stopped past [`CALL_TIMEOUT`]; its reply, an error when
    /// it says one.
    pub fn call(&self, request: &Request) -> anyhow::Result<Reply> {
        let kept = kept();
        let contract = kept.contract(self);
        kept.answer(self, contract.as_ref(), request)
    }

    /// Runs `lnk-<name> call` with `request`, stopped past
    /// [`CALL_TIMEOUT`]: its reply, an error when it says one.
    pub fn call_once(&self, request: &Request) -> anyhow::Result<Reply> {
        let running = || format!("running lnk-{} call", module_for(&self.name));
        let mut child = link_secretive::process::ours(&self.program)
            .arg("call")
            .env(DIR_ENV, &self.dir)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(running)?;
        let pid = child.id();
        let (done, waiting) = std::sync::mpsc::channel::<()>();
        let watchdog = std::thread::spawn(move || {
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                waiting.recv_timeout(CALL_TIMEOUT)
            {
                // SAFETY: a signal to the child, not yet waited for.
                unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
            }
        });
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            // A driver that stops before reading says why on stderr.
            let _ = stdin.write_all(&serde_json::to_vec(request)?);
        }
        let output = child.wait_with_output();
        let _ = done.send(());
        let _ = watchdog.join();
        let output = output.with_context(running)?;
        let reply: Option<Reply> = serde_json::from_slice(&output.stdout).ok();
        match reply {
            Some(Reply::Error { error }) => {
                bail!(
                    "lnk-{}: {}",
                    module_for(&self.name),
                    link_format::clean(&error)
                )
            }
            Some(reply) if output.status.success() => Ok(reply),
            _ => bail!(
                "lnk-{} call failed ({}): {}",
                module_for(&self.name),
                output.status,
                link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
            ),
        }
    }
}

/// The drivers this process keeps running, as the store bridge keeps
/// them: at most [`BRIDGE_KEPT`] idle of each program, folder and
/// environment once it served, so a turn's calls to an agent's store
/// cost no process each, and each stopped by its sweep once idle for
/// [`BRIDGE_KEPT_IDLE`] ([`Running::sweep`]). Those left end with this
/// process, their requests' pipe closed.
fn kept() -> &'static Running {
    static KEPT: std::sync::LazyLock<std::sync::Arc<Running>> = std::sync::LazyLock::new(|| {
        let running = std::sync::Arc::new(Running::default());
        let never = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        Running::sweep(&running, never);
        running
    });
    &KEPT
}

impl Store for Driver {
    fn name(&self) -> &str {
        &self.name
    }

    fn call(&self, request: &Request) -> anyhow::Result<Reply> {
        Driver::call(self, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A driver that answers what it can't, as one bridged could: a
    /// fixed reply.
    struct Answering(Reply);

    impl Store for Answering {
        fn name(&self) -> &str {
            "lnk-answering"
        }

        fn call(&self, _: &Request) -> anyhow::Result<Reply> {
            Ok(self.0.clone())
        }
    }

    /// What a driver answers is checked: a list of more paths than the
    /// contract allows, a narrowing that widened, or none at all, and a
    /// tool that's no tool's name are each an error, never taken.
    #[test]
    fn a_drivers_answers_are_held_to_the_contract() {
        let agent = Agent::default();
        let many = Answering(Reply::Files {
            files: vec!["skills/x".into(); FILES_MAX + 1],
        });
        assert!(many.file_list(&agent, "skills/").is_err());
        let enough = Answering(Reply::Files {
            files: vec!["skills/x".into(); FILES_MAX],
        });
        assert_eq!(
            enough.file_list(&agent, "skills/").unwrap().len(),
            FILES_MAX
        );

        let asked = ["web__search".to_string()];
        let wider = Answering(Reply::Tools {
            tools: Some(vec!["web__search".into(), "web__fetch".into()]),
        });
        assert!(wider.tools_narrow(&agent, "t", &asked).is_err());
        let every = Answering(Reply::Tools { tools: None });
        assert!(every.tools_narrow(&agent, "t", &asked).is_err());
        let narrow = Answering(Reply::Tools {
            tools: Some(Vec::new()),
        });
        assert_eq!(
            narrow.tools_narrow(&agent, "t", &asked).unwrap(),
            Vec::<String>::new()
        );
        let odd = Answering(Reply::Tools {
            tools: Some(vec!["not a tool".into()]),
        });
        assert!(odd.tools_read(&agent, "t").is_err());
        assert_eq!(every.tools_read(&agent, "t").unwrap(), None);
        // One that doesn't keep a conversation's tools: an error.
        let done = Answering(Reply::Done);
        assert!(done.tools_read(&agent, "t").is_err());
        // A write answered with anything but its version, or the other
        // way round: an error.
        assert!(
            done.file_write(&agent, "instructions/a.md", b"x", 0)
                .is_err()
        );
        assert!(done.file_append(&agent, "instructions/a.md", b"x").is_err());
        assert!(done.file_remove(&agent, "instructions/a.md", 1).is_err());
        let stale = Answering(Reply::Stale { version: 3 });
        assert_eq!(
            stale
                .file_write(&agent, "instructions/a.md", b"x", 2)
                .unwrap(),
            Written::Stale(3)
        );
        assert!(
            stale
                .file_append(&agent, "instructions/a.md", b"x")
                .is_err()
        );
    }

    /// A driver runs with what its caller sets in its environment: its
    /// settings, where its own docs say it reads them.
    #[test]
    fn a_driver_runs_with_its_environment() {
        let dir = link_test_kit::TempDir::new("store-driver-env");
        let program = dir.join("lnk-env");
        link_test_kit::write_program(
            &program,
            "#!/bin/sh
echo \"{\\\"reply\\\":\\\"files\\\",\\\"files\\\":[\\\"$LNK_ENV_SETTINGS\\\"]}\"\n",
        )
        .unwrap();
        let driver = Driver::at("env", program, &dir).with_env("LNK_ENV_SETTINGS", "skills/x");
        assert_eq!(
            driver.file_list(&Agent::default(), "skills/").unwrap(),
            ["skills/x"]
        );
    }

    /// A driver whose contract says `serve` is started once for a
    /// conversation's many calls, not once a call: its contract asked
    /// once, then one process answering them all.
    #[test]
    fn a_serving_driver_is_one_process_for_many_calls() {
        let dir = link_test_kit::TempDir::new("store-driver-kept");
        let program = dir.join("lnk-counted");
        let started = dir.join("started");
        link_test_kit::write_program(
            &program,
            format!(
                "#!/bin/sh\n\
                 echo \"$1\" >> '{}'\n\
                 case \"$1\" in\n\
                 contract) echo '{{\"version\":{VERSION},\"serve\":true}}' ;;\n\
                 serve) n=0\n\
                   while read -r line; do\n\
                     n=$((n+1))\n\
                     echo \"{{\\\"reply\\\":\\\"last\\\",\\\"last\\\":$n}}\"\n\
                   done ;;\n\
                 *) exit 2 ;;\n\
                 esac\n",
                started.display()
            ),
        )
        .unwrap();
        let driver = Driver::at("counted", program, &dir);
        // A pool of its own, its drivers stopped with it.
        let running = Running::default();
        let last = Request::Last {
            agent: Agent::default(),
            thread: "t".into(),
        };
        for n in 1..=50 {
            assert_eq!(
                running.call(&driver, &last).unwrap(),
                Reply::Last { last: n }
            );
        }
        assert_eq!(super::super::tests::lines(&started), ["contract", "serve"]);
    }

    #[test]
    fn a_drivers_name_is_a_plain_word() {
        for good in ["sqlite", "postgres-2", "my-store"] {
            assert!(check_name(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "log",
            "compact",
            "Sqlite",
            "a/b",
            "2db",
            "a b",
            &"x".repeat(65),
        ] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    /// A driver that stops, says nothing, or says an error is an error,
    /// never a reply.
    #[test]
    fn a_failing_driver_is_an_error() {
        let dir = std::env::temp_dir().join(format!("lnk-store-failing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let agent = Agent::default();
        for (name, script, says) in [
            ("silent", "#!/bin/sh\nexit 0\n", "call failed"),
            ("crashing", "#!/bin/sh\necho broken >&2\nexit 3\n", "broken"),
            (
                "erring",
                "#!/bin/sh\necho '{\"reply\":\"error\",\"error\":\"disk full\"}'\n",
                "disk full",
            ),
            (
                "odd",
                "#!/bin/sh\necho '{\"reply\":\"lost\"}'\n",
                "something else",
            ),
        ] {
            let program = dir.join(format!("lnk-{name}"));
            link_test_kit::write_program(&program, script).unwrap();
            let driver = Driver::at(name, program, &dir);
            let err = driver.last(&agent, "t").unwrap_err();
            assert!(format!("{err:#}").contains(says), "{name}: {err:#}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
