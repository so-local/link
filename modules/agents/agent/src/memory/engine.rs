//! An engine found or installed, and confined to what it reads and
//! writes when it serves.

use super::*;
use link_format::WithinExt;
use link_memory_tool_contract::declared::{Launch, LaunchShowArgs, MemoryInstallArgs};

/// A memory engine's adapter, `lnk-<module>`.
pub struct Engine {
    pub name: String,
    pub(super) program: PathBuf,
}

/// Whom an engine serves: its opinions (a spec's memory table, as JSON),
/// and whether it runs confined.
pub(super) struct Serving {
    pub(super) spec: String,
    pub(super) sandbox: bool,
    /// Whether the agent's harness is sent messages from others (its
    /// contract's `from`), so its memory notes only in the owner's
    /// conversations ([`FROM_ENV`]).
    pub(super) from: bool,
}

impl Serving {
    pub(super) fn of(agent: &Settings) -> anyhow::Result<Self> {
        Ok(Self {
            spec: serde_json::to_string(&agent.spec()?.tool("memory"))?,
            sandbox: agent.sandbox,
            // A harness that can't say is taken to be sent them.
            from: match link_agent::adapter::Adapter::find(agent.harness()) {
                Ok(adapter) => adapter.contract()?.from,
                Err(_) => true,
            },
        })
    }
}

impl Engine {
    /// The adapter of engine `name`, installing its module first (`lnk
    /// module add`) if it's one Link knows and it's missing: said first,
    /// for `whose` memory, and asked in a terminal unless it was `chosen`.
    pub fn find_or_add(
        out: &mut Out,
        name: &str,
        whose: &str,
        chosen: bool,
    ) -> Result<Self, Report> {
        link_machine::machine::check_name(name)
            .within(format!("{name:?} isn't a memory engine's name"))?;
        let module = settings::memory_module_of(name);
        let known = settings::MEMORIES.iter().any(|(k, _, _)| *k == name);
        if known && link_core::find_module(module).is_none() {
            let shown = Name::new("module", format!("lnk-{module}"));
            out.block(Note(sentence!(
                "{} is kept by its engine {}, which isn't on this machine: installing it from \
                 Link's release.",
                Foreign::new(whose),
                &shown
            )));
            let add = cmd!("lnk module add {}", Name::new("module", module));
            if !chosen
                && crate::tools::may_ask()
                && !out.ask(Question::confirm(sentence!("Install it now?"), true))?
            {
                return Err(
                    Report::error(sentence!("{} isn't installed.", &shown)).next(add, "install it")
                );
            }
            out.hand_over();
            let status = link_core::lnk()
                .args(["module", "add", module])
                .status()
                .within("running lnk module add")?;
            if !status.success() {
                return Err(Report::error(sentence!(
                    "Couldn't install the {} module.",
                    Name::new("module", module)
                ))
                .next(add, "try again"));
            }
        }
        let Some(program) = link_core::find_module(module) else {
            let known: Vec<Value> = settings::MEMORIES
                .iter()
                .map(|(k, _, _)| Value::from(*k))
                .collect();
            return Err(Report::error(sentence!(
                "There is no memory engine {} next to lnk or on your PATH.",
                Name::new("engine", format!("{name} (lnk-{module})"))
            ))
            .why(prose!("Link's are {}.", Value::List(known))));
        };
        Ok(Self {
            name: name.to_string(),
            program,
        })
    }

    /// `lnk-<module> <args>` (a line the protocol's generated arguments
    /// wrote), with the state folder as its HOME, confined
    /// unless the agent's sandbox is off: it writes only `state`, reads
    /// its own program and `read`, and reaches no network.
    /// `notes`, the agent's notes file when it may write them, is in its
    /// environment ([`NOTES_ENV`]).
    fn run(
        &self,
        serving: &Serving,
        state: &Path,
        read: &[&Path],
        notes: Option<&Path>,
        args: Vec<String>,
    ) -> anyhow::Result<Command> {
        let mut argv = vec![self.program.display().to_string()];
        argv.extend(args);
        // Its opinions, from the agent's spec: memory's table, never
        // dropped for the engine's own.
        let mut env = vec![(link_memory_tool_contract::SPEC_ENV, serving.spec.clone())];
        if let Some(notes) = notes {
            env.push((NOTES_ENV, notes.display().to_string()));
            if serving.from {
                env.push((FROM_ENV, "1".to_string()));
            }
        }
        let mut command = match serving.sandbox {
            true => {
                let env: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
                let mut command = link_core::lnk();
                command.args(confined(&self.program, state, read, &env, &argv));
                command
            }
            false => {
                let mut command = link_secretive::process::command(&argv[0]);
                command.args(&argv[1..]).env("HOME", state);
                command.envs(env);
                command
            }
        };
        command.current_dir(state);
        Ok(command)
    }

    /// Installs the engine if it isn't; output goes to the terminal. It
    /// has no network: a cloud's machine refuses a sandbox the machine's
    /// whole network, and an engine is installed as a module is, signed,
    /// before it runs, so its install fetches nothing.
    pub(super) fn install(&self, serving: &Serving, state: &Path) -> anyhow::Result<()> {
        let status = self
            .installing(serving, state)?
            .status()
            .within(format!("running lnk-{} install", self.name))?;
        if !status.success() {
            bail!("installing memory engine {} failed ({status})", self.name);
        }
        Ok(())
    }

    /// The command [`Engine::install`] runs: confined, writing `state`,
    /// with no network.
    fn installing(&self, serving: &Serving, state: &Path) -> anyhow::Result<Command> {
        self.run(serving, state, &[], None, MemoryInstallArgs {}.argv())
    }

    /// How it serves `folder`, with the agent's own folder `own` (its
    /// conversations and skills), keeping its state in `state`, writing
    /// the agent's notes at `notes` when it may.
    pub(super) fn serve(
        &self,
        serving: &Serving,
        folder: &Path,
        own: Option<&Path>,
        state: &Path,
        notes: Option<&Path>,
    ) -> anyhow::Result<Launch> {
        let args = LaunchShowArgs {
            folder: folder.to_path_buf(),
            state: state.to_path_buf(),
            agent: own.map(Path::to_path_buf),
        }
        .argv();
        let own_reads = own.map(own_reads).unwrap_or_default();
        let mut read: Vec<&Path> = vec![folder];
        read.extend(own_reads.iter().map(PathBuf::as_path));
        let output = self
            .run(serving, state, &read, notes, args)?
            .stderr(Stdio::inherit())
            .output()
            .within(format!("running lnk-{} command", self.name))?;
        if !output.status.success() {
            bail!("lnk-{} command failed ({})", self.name, output.status);
        }
        let serve: Launch = serde_json::from_slice(&output.stdout)
            .within(format!("reading what lnk-{} command printed", self.name))?;
        if serve.argv.is_empty() {
            bail!("lnk-{} command named no program", self.name);
        }
        // What it names is what its sandbox reads, as its own program:
        // only itself, never a folder or another's program.
        let named = Path::new(&serve.argv[0]).canonicalize().ok();
        if named.is_none() || named != self.program.canonicalize().ok() {
            bail!(
                "lnk-{} command named {} to serve, not itself: refused",
                self.name,
                link_format::clean(&serve.argv[0])
            );
        }
        Ok(serve)
    }
}

/// `lnk sandbox run` of `argv`, whose program is `program`, with HOME
/// `state` and the variables `env` (`NAME=value`): `lnk`'s arguments. It
/// reads that program alone, not the folder it's in (`~/.lnk/exec/bin`,
/// with `lnk` and every module), and reaches no network.
pub(crate) fn confined(
    program: &Path,
    state: &Path,
    read: &[&Path],
    env: &[String],
    argv: &[String],
) -> Vec<String> {
    confined_writing(program, state, &[], read, &Network::Closed, env, argv)
}

/// What a confined engine reaches of the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Network {
    /// Nothing.
    Closed,
    /// This machine's model runtimes on these ports, for inference only:
    /// the embedding models served for it outside. On Linux, through the
    /// proxy's local-model route at their own addresses
    /// (`[localhost.linux] models`), which a cloud's machine takes; on
    /// macOS, the machine's network as it is (`[network.macos] mode =
    /// "host"`), where a sandbox keeps a runtime's address only that way.
    Models(Vec<Port>),
    /// The machine's network as it is (`[network] mode = "host"`): an
    /// engine that reaches this machine's runtimes (`models`) with none
    /// served for it, at ports Link doesn't know. A cloud's machine
    /// refuses it.
    Host,
}

/// A model runtime's port, as [`Network::Models`] gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Port {
    /// Known now: the service's models, served before it starts.
    At(u16),
    /// The model `lnk agent memory serve` serves before the engine
    /// starts, its port filled in there ([`super::embedding::serve`]).
    Served,
}

impl Network {
    /// Its `lnk sandbox run` flags.
    fn flags(&self) -> Vec<String> {
        match self {
            Network::Closed => Vec::new(),
            Network::Models(ports) if ports.is_empty() => Vec::new(),
            Network::Models(ports) => {
                let mut flags: Vec<String> = ports
                    .iter()
                    .flat_map(|port| {
                        let port = match port {
                            Port::At(port) => port.to_string(),
                            Port::Served => super::embedding::port_placeholder(),
                        };
                        [MODELS_FLAG.to_string(), port]
                    })
                    .collect();
                flags.extend(["--network.macos.mode".into(), "host".into()]);
                flags
            }
            Network::Host => vec!["--network.mode".into(), "host".into()],
        }
    }
}

/// The flag naming a model runtime's port on Linux, which
/// [`super::embedding::serve`] fills in or drops.
pub(super) const MODELS_FLAG: &str = "--localhost.linux.models";

/// [`confined`], writing `write` too, each a folder inside one of `read`,
/// and reaching `network`.
pub(super) fn confined_writing(
    program: &Path,
    state: &Path,
    write: &[&Path],
    read: &[&Path],
    network: &Network,
    env: &[String],
    argv: &[String],
) -> Vec<String> {
    // What it runs, named: `env`, which sets its variables, and its program.
    let mut args: Vec<String> = vec![
        "sandbox".into(),
        "run".into(),
        "--commands.allow".into(),
        "/usr/bin/env".into(),
        "--commands.allow".into(),
        program.display().to_string(),
    ];
    for path in std::iter::once(state).chain(write.iter().copied()) {
        args.extend(["--files.write".into(), path.display().to_string()]);
    }
    for path in std::iter::once(program).chain(read.iter().copied()) {
        args.extend(["--files.read".into(), path.display().to_string()]);
    }
    args.extend(network.flags());
    args.extend([
        "--".into(),
        "/usr/bin/env".into(),
        format!("HOME={}", state.display()),
    ]);
    args.extend(env.iter().cloned());
    args.extend(argv.iter().cloned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An engine that names another program to serve, whose path its
    /// sandbox would read as its own, is refused; one naming itself isn't.
    #[test]
    fn an_engine_serves_only_as_itself() {
        let dir = std::env::temp_dir().join(format!("lnk-engine-argv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("lnk-fake-memory");
        let engine = Engine {
            name: "fake-memory".into(),
            program: program.clone(),
        };
        let serving = Serving {
            spec: "{}".into(),
            sandbox: false,
            from: false,
        };
        for (names, ok) in [
            (program.display().to_string(), true),
            ("/home".into(), false),
        ] {
            link_test_kit::write_program(
                &program,
                format!("#!/bin/sh\nprintf '{{\"argv\":[\"%s\",\"serve\"]}}' '{names}'\n"),
            )
            .unwrap();
            let served = engine.serve(&serving, &dir, None, &dir, None);
            assert_eq!(served.is_ok(), ok, "{names}: {served:?}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A confined engine installs with no network, so memory installs on
    /// a cloud's machine, whose sandbox refuses the machine's network.
    #[test]
    fn a_confined_install_asks_for_no_network() {
        let engine = Engine {
            name: "fake-memory".into(),
            program: PathBuf::from("/home/me/.lnk/exec/bin/lnk-fake-memory"),
        };
        let serving = Serving {
            spec: "{}".into(),
            sandbox: true,
            from: false,
        };
        let command = engine.installing(&serving, Path::new("/state")).unwrap();
        let args: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[..2], ["sandbox", "run"], "{args:?}");
        assert!(!args.iter().any(|a| a.starts_with("--network")), "{args:?}");
        assert_eq!(args.last().map(String::as_str), Some("install"), "{args:?}");
    }

    #[test]
    fn the_engine_reads_its_program_not_the_folder_it_is_in() {
        let program = Path::new("/home/me/.lnk/exec/bin/lnk-memory-tool");
        let args = confined(
            program,
            Path::new("/state"),
            &[Path::new("/files")],
            &[model_placeholder()],
            &["serve".into()],
        );
        let reads: Vec<&str> = args
            .windows(2)
            .filter(|w| w[0] == "--files.read")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(reads, ["/home/me/.lnk/exec/bin/lnk-memory-tool", "/files"]);
        let after = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(
            args[after..],
            [
                "--",
                "/usr/bin/env",
                "HOME=/state",
                "LNK_MEMORY_MODEL=",
                "serve"
            ]
        );
    }

    /// A served model is reached at its port alone through the proxy on
    /// Linux, never the machine's whole network, which a cloud's machine
    /// refuses; only an engine finding runtimes itself has that.
    #[test]
    fn a_served_model_is_reached_at_its_port_alone() {
        let flags = |network: Network| {
            confined_writing(
                Path::new("/p"),
                Path::new("/state"),
                &[],
                &[],
                &network,
                &[],
                &["serve".into()],
            )
            .into_iter()
            .take_while(|a| a != "--")
            .skip_while(|a| !a.starts_with("--network") && !a.starts_with("--localhost"))
            .collect::<Vec<_>>()
        };
        assert!(flags(Network::Closed).is_empty());
        assert!(flags(Network::Models(Vec::new())).is_empty());
        assert_eq!(
            flags(Network::Models(vec![Port::At(41234), Port::Served])),
            [
                MODELS_FLAG,
                "41234",
                MODELS_FLAG,
                &super::super::embedding::port_placeholder(),
                "--network.macos.mode",
                "host"
            ]
        );
        assert_eq!(flags(Network::Host), ["--network.mode", "host"]);
    }
}
