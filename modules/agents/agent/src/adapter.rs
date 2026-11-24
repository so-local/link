//! Running a harness program (`lnk-harness`, `lnk-<name>`) through
//! the commands in `link_harness_contract::harness`, and the home every one of them
//! runs in.

use link_format::WithinExt;
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{bail, ensure};
use link_harness_contract::harness::{Launch, Settings};

use std::collections::BTreeSet;

use link_agent_contract::agent_sandbox::Route;
use link_sandbox_client::{At, Confinement, Net, ProxyAt, Way};
use link_sandbox_contract::spec::{Files, Localhost, Port, Spec};

use crate::settings;
mod boxing;
mod contract;
mod home;
mod launches;
mod linking;
mod schedules;
mod step;
mod steps;
pub use boxing::Boxed;
use boxing::health_confinement;
pub use home::{AGENT_ENV, measures_of};
use home::{fresh, path, runtime_asked, write_shims};
pub use step::Step;
mod programs;
use launches::blocked_hint;
pub use launches::check_launch;
use steps::{hand, installed_module, machine_network, needs_of, offline_spec, step_spec};

pub struct Adapter {
    pub name: String,
    program: PathBuf,
}

/// What an adapter's program answered, shared by every [`Adapter`] of it
/// in this process (a start, `use` and `connect` each make several), and
/// asked again once the program changes: an adapter added again or
/// updated in place, while a long-lived process runs.
#[derive(Default)]
struct Answers {
    /// What `contract` said.
    contract: std::sync::OnceLock<contract::Answer>,
    /// What `hosts` said.
    hosts: std::sync::OnceLock<Vec<String>>,
    /// What `connections` said.
    connections: std::sync::OnceLock<Vec<String>>,
    /// What `endpoint` said for each base port.
    endpoints: std::sync::Mutex<BTreeMap<Option<u16>, Option<String>>>,
    /// What `needs` said, for each input it was given.
    needs: std::sync::Mutex<BTreeMap<Vec<u8>, link_harness_contract::harness::Needs>>,
}

impl Answers {
    /// Those of the program at `program` as it is now, made the first
    /// time they're asked for: kept by its path and its file's identity,
    /// size and time, so a program replaced or rewritten is asked again.
    /// Kept by nothing for a program that can't be read.
    fn of(program: &Path) -> std::sync::Arc<Self> {
        type Key = (PathBuf, u64, u64, u64, i64, i64);
        static ALL: std::sync::Mutex<BTreeMap<Key, std::sync::Arc<Answers>>> =
            std::sync::Mutex::new(BTreeMap::new());
        let Ok(meta) = std::fs::metadata(program) else {
            return Default::default();
        };
        #[cfg(unix)]
        let key = {
            use std::os::unix::fs::MetadataExt;
            let (dev, ino, len) = (meta.dev(), meta.ino(), meta.len());
            (
                program.to_path_buf(),
                dev,
                ino,
                len,
                meta.mtime(),
                meta.mtime_nsec(),
            )
        };
        #[cfg(not(unix))]
        let key = {
            let since = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .unwrap_or_default();
            let (secs, nanos) = (since.as_secs() as i64, since.subsec_nanos() as i64);
            (program.to_path_buf(), 0, 0, meta.len(), secs, nanos)
        };
        match ALL.lock() {
            Ok(mut all) => all.entry(key).or_default().clone(),
            Err(_) => Default::default(),
        }
    }
}

impl Adapter {
    /// The adapter for harness `name`: its module's program (`lnk-harness`
    /// for `link`, [`settings::KNOWN`]), else `lnk-<name>`, next to lnk or on
    /// the PATH.
    pub fn find(name: &str) -> anyhow::Result<Self> {
        // A name no harness here answers to is what the person gave,
        // refused naming it: never a fault of Link's to report.
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(link_format::refused(anyhow::anyhow!(
                "{name:?} is not a harness name"
            )));
        }
        let module = settings::module_of(name);
        let Some(program) = link_core::find_module(module) else {
            let known: Vec<_> = settings::KNOWN.iter().map(|(k, _, _)| *k).collect();
            return Err(link_format::refused(anyhow::anyhow!(
                "no harness {name} here (lnk-{module}, next to lnk or on your PATH). Link's are \
                 {}; a harness from outside Link is added from its repository: lnk module add --url \
                 <its URL>, known ones are at {}. lnk agent harness list lists those here",
                known.join(", "),
                settings::MORE_HARNESSES
            )));
        };
        Ok(Self {
            name: name.to_string(),
            program,
        })
    }

    /// The adapter for harness `name`, installing its module (`lnk module
    /// add <module>`) first if it's one Link comes with and it's missing.
    pub fn find_or_add(name: &str) -> anyhow::Result<Self> {
        let known = settings::known_harness(name);
        let module = settings::module_of(name);
        if known {
            link_core::modules::add(module, &format!("The {name} harness"))?;
        }
        Self::find(name)
    }

    /// Its program, `lnk-<name>`.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// `lnk-<name> <command>`, in the home.
    fn run(&self, boxed: &Boxed, command: &str) -> anyhow::Result<Command> {
        self.run_with(boxed, &[command])
    }

    /// `lnk-<name> <args>`, in the home. With the sandbox and the internet,
    /// behind a proxy of its own (the step's sockets apart, as a health
    /// check can run during another step), direct: `install` and
    /// `configure` reach only the adapter's hosts, if it names them.
    /// Wrapped once, it runs as often as it's needed.
    fn step(&self, boxed: &Boxed, args: &[&str]) -> anyhow::Result<Step> {
        let step = args.first().copied().unwrap_or_default();
        self.program_step(boxed, &self.program, args, step)
    }

    /// `program <args>` in the home, sandboxed as the adapter's `step`
    /// is ([`Self::step`]).
    fn program_step(
        &self,
        boxed: &Boxed,
        program: &Path,
        args: &[&str],
        step: &str,
    ) -> anyhow::Result<Step> {
        self.program_step_with(boxed, program, args, step, &[], &[])
    }

    /// [`Self::program_step`], writing `write` too, and running what it
    /// writes in `runs` of them ([`programs::runs_written`]).
    fn program_step_with(
        &self,
        boxed: &Boxed,
        program: &Path,
        args: &[&str],
        step: &str,
        write: &[PathBuf],
        runs: &[PathBuf],
    ) -> anyhow::Result<Step> {
        let (program, read) = self.step_reads(program);
        let directory = programs::directory(self, boxed)?;
        let mut read = read;
        if directory.is_dir() {
            read.push(directory.clone());
        }
        let argv: Vec<String> = std::iter::once(program.display().to_string())
            .chain(args.iter().map(|a| a.to_string()))
            .collect();
        let profile = format!("{}-setup", boxed.profile);
        let mut env = boxed.env();
        env.insert(
            link_files::layout::exec_variable().into(),
            directory.display().to_string(),
        );
        if !boxed.sandbox && ["install", "configure"].contains(&step) {
            machine_network(&mut env);
        }
        let argv = match boxed.wires.as_ref().filter(|_| boxed.sandbox) {
            None => boxed.wrap(&argv, &profile, write, &read, false, runs)?,
            Some(wires) => {
                let spec = step_spec(boxed, step, || self.hosts());
                let at = ProxyAt {
                    upstream: "direct".into(),
                    dir: wires.join(format!("{}-{step}", boxed.name)),
                    ..ProxyAt::default()
                };
                let mut confinement = boxed.confinement(
                    spec,
                    &program,
                    &profile,
                    write,
                    &read,
                    false,
                    Way::Proxy(at),
                )?;
                boxing::run_written(&mut confinement, runs);
                let wrapped = link_sandbox_client::wrap(&confinement, &argv)?;
                env.extend(wrapped.env);
                wrapped.argv
            }
        };
        Ok(Step {
            argv,
            env,
            dir: boxed.dir.clone(),
        })
    }

    /// `program` as a step runs it, and what the step reads outside: the
    /// adapter's own program (and a connection's module, its own), never the
    /// folder it's in, which holds whatever else the user keeps there.
    /// Each by its real path: the sandbox doesn't open the folder a link
    /// to it is in.
    fn step_reads(&self, program: &Path) -> (PathBuf, Vec<PathBuf>) {
        let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let program = real(program);
        let mut read = vec![real(&self.program), program.clone()];
        read.dedup();
        (program, read)
    }

    /// `lnk-<name> <args>`, in the home ([`Adapter::step`]).
    fn run_with(&self, boxed: &Boxed, args: &[&str]) -> anyhow::Result<Command> {
        Ok(self.step(boxed, args)?.command())
    }

    /// `program <args>`, in the home ([`Adapter::program_step`]).
    fn run_program(
        &self,
        boxed: &Boxed,
        program: &Path,
        args: &[&str],
        step: &str,
    ) -> anyhow::Result<Command> {
        Ok(self.program_step(boxed, program, args, step)?.command())
    }

    /// Installs the harness if it isn't; output goes to the terminal. A
    /// harness that downloads a runtime of its own ([`settings::runtime_of`])
    /// is asked about first in a fresh home, with what it downloads, and
    /// without a terminal installed only with `--yes`. A harness whose
    /// `needs` say its install runs what it downloads
    /// ([`link_harness_contract::harness::Needs::install_runs_downloads`])
    /// runs it in the folder it installs into, and nowhere else it writes,
    /// and that's said first.
    pub fn install(
        &self,
        boxed: &Boxed,
        needs: &link_harness_contract::harness::Needs,
        yes: bool,
    ) -> anyhow::Result<()> {
        let Some(installation) = programs::Installation::prepare(self, boxed)? else {
            return Ok(());
        };
        let write = std::slice::from_ref(&installation.staged);
        let runs = programs::runs_written("install", needs, write);
        if boxed.sandbox && !runs.is_empty() {
            link_format::aside(link_format::Note(link_format::sentence!(
                "Installing {} runs the programs it downloads into its own folder while it \
                 installs.",
                link_format::Name::new("harness", &self.name)
            )));
        }
        if let Some(what) = settings::runtime_of(&self.name)
            && fresh(&boxed.dir)
        {
            let terminal = std::io::IsTerminal::is_terminal(&std::io::stdin());
            runtime_asked(&self.name, what, yes, terminal, || {
                let asked = link_format::Question::confirm(
                    link_format::Sentence::adopt(&format!(
                        "{} downloads {what} the first time. Install it now?",
                        self.name
                    )),
                    true,
                )
                .flag("--yes");
                link_format::ask_aside(asked).map_err(|r| anyhow::anyhow!("{}", r.headline))
            })?;
        }
        let status = self
            .program_step_with(boxed, &self.program, &["install"], "install", write, runs)?
            .command()
            .env(link_files::layout::exec_variable(), &installation.staged)
            .status()
            .within(format!("running lnk-{} install", self.name))?;
        if !status.success() {
            bail!(
                "installing {} failed ({status}){}",
                self.name,
                blocked_hint(boxed, &status)
            );
        }
        installation.publish()
    }

    /// Hands the harness the settings (on stdin: they hold secrets).
    pub fn configure(&self, boxed: &Boxed, settings: &Settings) -> anyhow::Result<()> {
        let mut child = self
            .run(boxed, "configure")?
            .stdin(Stdio::piped())
            .spawn()
            .within(format!("running lnk-{} configure", self.name))?;
        hand(&mut child, &serde_json::to_vec(settings)?)?;
        let status = child.wait()?;
        if !status.success() {
            bail!(
                "configuring {} failed ({status}){}",
                self.name,
                blocked_hint(boxed, &status)
            );
        }
        Ok(())
    }

    /// Installs connection `connection`'s program into the home, for a harness
    /// speaking it (`lnk-<connection> cli install --home <home> --json`, its
    /// settings `section` on stdin), in the sandbox `configure` runs in,
    /// reaching the adapter's hosts: nothing the harness can write runs
    /// outside its sandbox. What the module printed, the program's path
    /// (`{"program"}`).
    pub fn connection_install(
        &self,
        boxed: &Boxed,
        connection: &str,
        section: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let program = link_core::find_module(connection).within(format!(
            "the {connection} module (lnk-{connection}) isn't installed"
        ))?;
        let home = boxed.dir.display().to_string();
        let args = ["cli", "install", "--home", &home, "--json"];
        let mut child = self
            .run_program(boxed, &program, &args, "configure")?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .within(format!("running lnk-{connection} cli install"))?;
        hand(&mut child, &serde_json::to_vec(section)?)?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!(
                "installing {connection}'s program for {} failed ({}){}",
                self.name,
                output.status,
                blocked_hint(boxed, &output.status)
            );
        }
        serde_json::from_slice(&output.stdout)
            .within(format!("reading lnk-{connection} cli install --json"))
    }

    /// Links connection `connection`'s account in the home, for a harness
    /// speaking it, with the program its module installed there
    /// (`lnk-<connection> cli link --home <home>`), in the harness's
    /// sandbox; as [`Self::link`].
    pub fn connection_link(
        &self,
        boxed: &Boxed,
        connection: &str,
        again: bool,
    ) -> anyhow::Result<bool> {
        let program = link_core::find_module(connection).within(format!(
            "the {connection} module (lnk-{connection}) isn't installed"
        ))?;
        let home = boxed.dir.display().to_string();
        let mut args = vec!["cli", "link", "--home", &home];
        if again {
            args.push("--again");
        }
        let status = self
            .run_program(boxed, &program, &args, "link")?
            .status()
            .within(format!("running lnk-{connection} cli link"))?;
        match status.code() {
            Some(0) => Ok(true),
            Some(link_harness_contract::harness::NOT_LINKED) => Ok(false),
            _ => bail!(
                "linking {connection} on {} failed ({status}){}",
                self.name,
                blocked_hint(boxed, &status)
            ),
        }
    }

    /// How to run the harness in the foreground.
    pub fn command(&self, boxed: &Boxed) -> anyhow::Result<Launch> {
        let output = self
            .run(boxed, "command")?
            .stderr(Stdio::inherit())
            .output()
            .within(format!("running lnk-{} command", self.name))?;
        if !output.status.success() {
            bail!("lnk-{} command failed ({})", self.name, output.status);
        }
        let mut launch: Launch = serde_json::from_slice(&output.stdout)
            .within(format!("lnk-{} command printed something else", self.name))?;
        if launch.argv.is_empty() {
            bail!("lnk-{} command gave nothing to run", self.name);
        }
        // Its own program is read as every step reads it ([`Self::step_reads`]),
        // wherever it's installed, even in the home folder (`~/.lnk/exec/bin`).
        let own = self
            .program
            .canonicalize()
            .unwrap_or_else(|_| self.program.clone());
        let mut checked = launch.clone();
        checked.read.retain(|p| {
            let real = p.canonicalize().unwrap_or_else(|_| p.clone());
            real != own && *p != self.program && !installed_module(&real)
        });
        check_launch(&checked, boxed)
            .within(format!("lnk-{} command asked for too much", self.name))?;
        let directory = programs::directory(self, boxed)?;
        if directory.is_dir() {
            launch.read.push(directory.clone());
        }
        launch.env.insert(
            link_files::layout::exec_variable().into(),
            directory.display().to_string(),
        );
        Ok(launch)
    }

    /// What `lnk-<name> instructions` prints (`--json` with `json`): the
    /// prompt its harness sends its model now. It runs in the home, the
    /// files folder and its notes' folder read and nothing else of the
    /// agent's, with no network.
    pub fn instructions(&self, boxed: &Boxed, json: bool) -> anyhow::Result<Vec<u8>> {
        let args: &[&str] = match json {
            true => &["instructions", "--json"],
            false => &["instructions"],
        };
        self.of_files(boxed, args, false, &notes_read(&boxed.folder))
    }

    /// What the harness holds of its own of who the agent is, in ASP's
    /// portable forms (`lnk-<name> state export`), for an adapter whose
    /// contract says `identity`: run as [`Self::instructions`] is,
    /// reading only its home.
    pub fn exported(
        &self,
        boxed: &Boxed,
        drivers: &[PathBuf],
    ) -> anyhow::Result<link_harness_contract::asp::State> {
        let out = self.of_files(boxed, &["state", "export"], false, drivers)?;
        serde_json::from_slice(&out).within(format!("reading lnk-{} state export", self.name))
    }

    /// What `lnk-<name> conversation <thread>` or `conversations move
    /// <way>` (`args`) prints, as [`Self::instructions`] runs: with
    /// `write`, the agent's conversations written too, as its harness
    /// writes them. It runs `drivers`, the store drivers' programs, read.
    pub fn conversations(
        &self,
        boxed: &Boxed,
        args: &[&str],
        write: bool,
        drivers: &[PathBuf],
    ) -> anyhow::Result<Vec<u8>> {
        self.of_files(boxed, args, write, drivers)
    }

    /// `lnk-<name> <args>` in the home, Home and the agent's own folder's
    /// instructions, skills and conversations read, with `write` its
    /// conversations written, `read` read, and no network: what it prints.
    #[expect(
        clippy::disallowed_methods,
        reason = "runs the adapter with the environment of the agent's own spec"
    )]
    fn of_files(
        &self,
        boxed: &Boxed,
        args: &[&str],
        write: bool,
        read: &[PathBuf],
    ) -> anyhow::Result<Vec<u8>> {
        let (program, mut reads) = self.step_reads(&self.program);
        reads.push(boxed.folder.home.clone());
        reads.push(boxed.dir.clone());
        // With `write`, its conversations alone written, whatever harness
        // it is: they're what a step moves between ways of keeping them.
        let folder = &boxed.folder;
        let conversations = folder.of(link_store_contract::storage::Collection::Conversations);
        let writes: Vec<PathBuf> = if args.starts_with(&["state", "clear-schedules"]) {
            vec![boxed.dir.clone()]
        } else {
            match write {
                true => vec![conversations.clone()],
                false => Vec::new(),
            }
        };
        reads.extend(
            link_agent_contract::folder::SEEN
                .iter()
                .map(|c| folder.of(*c))
                .filter(|path| path.is_dir() && !writes.contains(path)),
        );
        reads.extend(read.iter().cloned());
        let step = args.first().copied().unwrap_or_default();
        let argv: Vec<String> = std::iter::once(program.display().to_string())
            .chain(args.iter().map(|a| a.to_string()))
            .collect();
        let argv = match boxed.sandbox {
            true => {
                // Nothing of the agent's grants but the programs its
                // harness may run ([`offline_spec`]): no network, not your
                // home.
                let mut confinement = boxed.confinement(
                    offline_spec(boxed),
                    &program,
                    &format!("{}-setup", boxed.profile),
                    &writes,
                    &reads,
                    false,
                    Way::Proxy(ProxyAt::default()),
                )?;
                // And the store drivers it's handed, which it runs.
                if let link_sandbox_client::Programs::Only(programs) = &mut confinement.commands {
                    programs.extend(read.iter().cloned());
                }
                link_sandbox_client::wrap(&confinement, &argv)?.argv
            }
            false => argv,
        };
        let output = Command::new(&argv[0])
            .args(&argv[1..])
            .env_clear()
            .envs(boxed.env())
            .current_dir(&boxed.dir)
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()
            .within(format!("running lnk-{} {step}", self.name))?;
        if !output.status.success() {
            bail!("lnk-{} {step} failed ({})", self.name, output.status);
        }
        Ok(output.stdout)
    }

    /// Its health check (`health`), wrapped once to run as often as
    /// needed: it succeeds when the running harness answers. Sandboxed, it
    /// reaches the harness's ports on this machine's loopback and nothing
    /// else: no proxy, so no keys on a wire, and no other network.
    pub fn health(&self, boxed: &Boxed) -> anyhow::Result<Step> {
        if !boxed.sandbox {
            return self.step(boxed, &["health"]);
        }
        let (program, read) = self.step_reads(&self.program);
        let argv = [program.display().to_string(), "health".into()];
        let wrapped =
            link_sandbox_client::wrap(&health_confinement(boxed, &program, &read)?, &argv)?;
        let mut env = boxed.env();
        env.extend(wrapped.env);
        Ok(Step {
            argv: wrapped.argv,
            env,
            dir: boxed.dir.clone(),
        })
    }

    /// Whether the running harness answers.
    pub fn healthy(&self, boxed: &Boxed) -> bool {
        self.health(boxed).is_ok_and(|step| step.succeeds())
    }

    /// Where the harness writes when it isn't boxed: `~/...` or absolute
    /// paths, `{uid}` standing for the user's id. Empty if the adapter
    /// doesn't say.
    /// What of its home is the agent (`lnk-<name> state`), for `lnk agent
    /// move`. An adapter without the command can't move.
    pub fn state(&self) -> anyhow::Result<link_harness_contract::harness::State> {
        let output = link_secretive::process::command(&self.program)
            .arg("state")
            .stderr(Stdio::null())
            .output()
            .within(format!("running lnk-{} state", self.name))?;
        if !output.status.success() {
            bail!(
                "lnk-{} can't say what of its home to move (no state command): update it",
                self.name
            );
        }
        serde_json::from_slice(&output.stdout)
            .within(format!("reading what lnk-{} state printed", self.name))
    }

    /// The connections its harness speaks (`lnk-<name> connections`): Telegram
    /// only for an adapter without the command.
    pub fn connections(&self) -> Vec<String> {
        Answers::of(&self.program)
            .connections
            .get_or_init(|| {
                link_secretive::process::command(&self.program)
                    .arg("connections")
                    .stderr(Stdio::null())
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .and_then(|o| serde_json::from_slice(&o.stdout).ok())
                    .unwrap_or_else(|| {
                        link_harness_contract::harness::FIRST_CONNECTIONS
                            .iter()
                            .map(|c| c.to_string())
                            .collect()
                    })
            })
            .clone()
    }

    /// Where its harness answers chat completions, when it speaks no
    /// connection itself (`lnk-<name> endpoint`): Link carries the connections
    /// whose modules carry messages for it. `port` is its agent's base
    /// port. None for an adapter without the command, or one that names
    /// no address on the loopback.
    pub fn endpoint(&self, port: Option<u16>) -> Option<String> {
        let answers = Answers::of(&self.program);
        if let Some(known) = answers.endpoints.lock().ok()?.get(&port) {
            return known.clone();
        }
        let asked = self.ask_endpoint(port);
        if let Ok(mut endpoints) = answers.endpoints.lock() {
            endpoints.insert(port, asked.clone());
        }
        asked
    }

    fn ask_endpoint(&self, port: Option<u16>) -> Option<String> {
        let mut command = link_secretive::process::command(&self.program);
        command.arg("endpoint").stderr(Stdio::null());
        match port {
            Some(port) => command.env(link_harness_contract::harness::PORT_ENV, port.to_string()),
            None => command.env_remove(link_harness_contract::harness::PORT_ENV),
        };
        let url: String = command
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| serde_json::from_slice(&o.stdout).ok())?;
        link_harness_contract::harness::endpoint_port(&url).ok()?;
        Some(url)
    }

    /// What agent `agent` can talk on with this harness: the connections it
    /// speaks, and with an endpoint, those Link carries for it.
    pub fn speaks(&self, agent: &settings::Settings) -> Speaking {
        Speaking {
            own: self.connections(),
            endpoint: self.endpoint(agent.port).is_some(),
        }
    }
    /// The hosts its `install` and `configure` reach (`lnk-<name>
    /// hosts`); empty, any, for an adapter without the command. Asked
    /// once, and only by the steps that use it.
    pub fn hosts(&self) -> Vec<String> {
        Answers::of(&self.program)
            .hosts
            .get_or_init(|| {
                link_secretive::process::command(&self.program)
                    .arg("internet-hosts")
                    .stderr(Stdio::null())
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .and_then(|o| serde_json::from_slice(&o.stdout).ok())
                    .unwrap_or_default()
            })
            .clone()
    }

    /// What its harness needs beyond Link's defaults (`lnk-<name>
    /// needs`), given `agent`'s settings without its keys on stdin.
    /// Nothing for an adapter without the command, whatever version of
    /// the contract it speaks: one that exits with an error (its usual
    /// answer to a command it doesn't know) or prints nothing. An error
    /// for an answer that isn't [`link_harness_contract::harness::Needs`].
    pub fn needs(
        &self,
        agent: &settings::Settings,
    ) -> anyhow::Result<link_harness_contract::harness::Needs> {
        let mut settings = agent.for_needs()?;
        // Whether it runs with no sandbox whatever it says (`--no-sandbox`).
        settings.unsandboxed = !agent.sandbox;
        // Whether behind its proxy, with what it has so far: the network
        // as it is that it asks for now is its own to count.
        settings.proxied = agent.sandbox && agent.proxy_of(&self.name).is_some();
        let input = serde_json::to_vec(&settings)?;
        // The same settings, the same answer: asked once.
        let answers = Answers::of(&self.program);
        if let Some(known) = answers
            .needs
            .lock()
            .ok()
            .and_then(|known| known.get(&input).cloned())
        {
            return Ok(known);
        }
        // Nothing of this process's environment but where programs and
        // home are: never a key or token Link was given (a provider's
        // `ANTHROPIC_API_KEY`, a connection's), which the contract says it doesn't get.
        let mut command = link_secretive::process::command(&self.program);
        command.env_clear();
        for kept in ["PATH", "HOME", "LANG", "TMPDIR"] {
            if let Some(value) = std::env::var_os(kept) {
                command.env(kept, value);
            }
        }
        let mut child = command
            .arg("needs")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .within(format!("running lnk-{} needs", self.name))?;
        let _ = hand(&mut child, &input);
        let output = child.wait_with_output()?;
        let needs = needs_of(&self.name, output.status.success(), &output.stdout)?;
        if let Ok(mut known) = answers.needs.lock() {
            known.insert(input, needs.clone());
        }
        Ok(needs)
    }

    pub fn traces(&self) -> Vec<link_harness_contract::harness::traces::Trace> {
        link_secretive::process::command(&self.program)
            .arg("traces")
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| serde_json::from_slice(&o.stdout).ok())
            .unwrap_or_default()
    }

    /// The version that wrote the install whose home is `home`, and why
    /// Link can't run it, as the adapter reads it (`written`).
    pub fn written(
        &self,
        home: &Path,
    ) -> anyhow::Result<link_harness_contract::harness::traces::Written> {
        let out = link_secretive::process::command(&self.program)
            .arg("written")
            .arg(home)
            .stderr(Stdio::null())
            .output()
            .within(format!("running {}", self.program.display()))?;
        anyhow::ensure!(
            out.status.success(),
            "{} can't say which version wrote {}: update it (lnk module update)",
            self.program.display(),
            home.display()
        );
        Ok(serde_json::from_slice(&out.stdout)?)
    }

    /// Secrets `values` taken out of the files of its own home `home`
    /// keeps them in (`clean`, for an adapter whose contract says so).
    pub fn forget(&self, home: &Path, values: &[&str]) -> anyhow::Result<()> {
        let mut child = link_secretive::process::command(&self.program)
            .arg("clean")
            .arg(home)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .within(format!("running {}", self.program.display()))?;
        hand(&mut child, &serde_json::to_vec(values)?)?;
        let status = child.wait()?;
        anyhow::ensure!(
            status.success(),
            "{} couldn't take secrets out of {} ({status})",
            self.program.display(),
            home.display()
        );
        Ok(())
    }
}

/// The folder `folder`'s notes are in, for a step that composes its
/// prompt, where it's there: memory writes them whole, so the folder.
fn notes_read(folder: &link_agent_contract::folder::Folder) -> Vec<PathBuf> {
    let notes = folder.of(link_store_contract::storage::Collection::Memory);
    notes.is_dir().then_some(notes).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `lnk agent show --instructions` composes the prompt with the agent's
    /// notes, as its harness does.
    #[test]
    fn the_prompt_printed_reads_the_notes() {
        let root = link_test_kit::TempDir::new("notes");
        let folder = link_agent_contract::folder::Folder {
            home: root.path().join("home"),
            agent: root.path().join("agent"),
        };
        assert!(notes_read(&folder).is_empty());
        let notes = folder
            .agent
            .join(link_harness_contract::harness::layout::NOTES);
        std::fs::create_dir_all(notes.parent().unwrap()).unwrap();
        assert_eq!(notes_read(&folder), [notes.parent().unwrap()]);
    }

    /// A harness not here is refused with the harnesses there are and
    /// how another is added, not a bare refusal of its name.
    #[test]
    fn a_harness_not_here_names_those_there_are_and_how_to_add_one() {
        let err = format!("{:#}", Adapter::find("nope").err().unwrap());
        assert!(
            err.contains("link, mini, openclaw, hermes, deepseek"),
            "{err}"
        );
        assert!(err.contains("lnk module add --url <its URL>"), "{err}");
        // `lnk` hands the agent any name of a harness's shape: none is
        // refused before by a list of values.
        let set = crate::cli::command();
        let set = set
            .find_subcommand("harness")
            .and_then(|h| h.find_subcommand("set"))
            .unwrap();
        let named = set
            .get_arguments()
            .find(|a| a.get_id() == "harness")
            .unwrap();
        assert!(named.get_possible_values().is_empty());
    }

    /// A harness name nothing here answers to is the person's to fix:
    /// refused as a usage error, never a fault of Link's to report.
    #[test]
    fn an_unknown_harness_is_refused_naming_it() {
        for name in ["qa-no-such-harness", "a/b"] {
            let err = Adapter::find(name).err().unwrap();
            let report = link_format::Report::from(err);
            assert!(!report.is_bug(), "{name}");
            assert_eq!(report.exit, link_format::Exit::Usage, "{name}");
            assert!(report.headline.to_string().contains(name), "{name}");
        }
    }

    #[test]
    fn mini_is_selected_by_its_harness_name_and_resolves_to_its_module() {
        use clap::Parser;
        assert_eq!(settings::module_of("mini"), "mini-harness");
        let crate::cli::Command::HarnessSet(options) =
            crate::cli::Cli::try_parse_from(["lnk-agent", "harness", "set", "mini"])
                .unwrap()
                .command
        else {
            panic!("agent harness set")
        };
        assert_eq!(options.harness, "mini");
        let error =
            crate::cli::Cli::try_parse_from(["lnk-agent", "harness", "set", "mini-harness"])
                .err()
                .unwrap();
        assert!(error.to_string().contains("name the harness"), "{error}");
    }

    /// Every `Adapter` of a program shares its answers, asked once; a
    /// program replaced, or rewritten in place, is asked again.
    #[test]
    #[cfg(unix)]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn answers_are_asked_once_a_program_and_again_when_it_changes() {
        let root = link_test_kit::TempDir::new("answers");
        let (program, log) = (root.path().join("lnk-h"), root.path().join("asked"));
        let script = |version: u32| {
            format!(
                "#!/bin/sh\n[ \"$1\" = ready ] && exit 0\necho \"$1\" >> '{}'\n\
                 echo '{{\"version\": {version}}}'\n",
                log.display()
            )
        };
        let adapter = || Adapter {
            name: "h".into(),
            program: program.clone(),
        };
        let asked = || {
            std::fs::read_to_string(&log)
                .unwrap_or_default()
                .lines()
                .count()
        };
        link_test_kit::write_program(&program, script(1)).unwrap();
        let (first, second) = (adapter(), adapter());
        assert_eq!(first.contract().unwrap().version, 1);
        assert_eq!(second.contract().unwrap().version, 1);
        assert_eq!(first.contract().unwrap().version, 1);
        assert_eq!(asked(), 1, "asked once for every Adapter of the program");
        // Replaced, as an install does.
        link_test_kit::write_program(&program, script(22)).unwrap();
        assert_eq!(first.contract().unwrap().version, 22);
        assert_eq!(adapter().contract().unwrap().version, 22);
        assert_eq!(asked(), 2);
        // Rewritten in place, then run once no other test's child, forked
        // as it was written, holds it open ("text file busy").
        std::fs::write(&program, script(333)).unwrap();
        for _ in 0..100 {
            match Command::new(&program).arg("ready").status() {
                Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                _ => break,
            }
        }
        assert_eq!(second.contract().unwrap().version, 333);
        assert_eq!(asked(), 3);
    }
}

/// What an agent can talk on with a harness ([`Adapter::speaks`]).
#[derive(Clone, Debug, Default)]
pub struct Speaking {
    /// The connections the harness speaks itself.
    pub own: Vec<String>,
    /// Whether it answers an endpoint, so Link carries the connections whose
    /// module carries messages (`describe`'s `bridged`).
    pub endpoint: bool,
}

impl Speaking {
    /// Whether agent `agent` can talk on `connection`: its harness speaks
    /// it, or Link carries it, as the connection's module said when it was
    /// connected, or says now (`lnk-<connection> describe`).
    pub fn speaks(&self, agent: &settings::Settings, connection: &str) -> bool {
        if self.own.iter().any(|c| c == connection) {
            return true;
        }
        self.endpoint
            && match agent.connections.get(connection) {
                Some(connected) => connected.about.bridged,
                None => crate::connections::describe(connection).is_ok_and(|d| d.bridged),
            }
    }
}
