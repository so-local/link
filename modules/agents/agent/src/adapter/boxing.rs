//! A harness's home as a sandbox: [`Boxed`], its environment, its spec
//! and its confinement, the health check's with it.

use super::*;
use link_format::WithinExt;

/// A harness's home: the one folder it lives in, its `HOME`. Everything of
/// the harness runs here: its installer, its configuration, the harness
/// itself. Sandboxed, none of it can write anywhere but the home, the files
/// folder (the harness itself only) and `/tmp/<name>*`.
pub struct Boxed {
    pub name: String,
    /// Its agent's name, in its programs' environment (`LNK_AGENT`).
    pub agent: String,
    /// Its agent's ID and owner, in its programs' environment
    /// (`LNK_AGENT_ID`, `LNK_OWNER`), when its settings have them.
    pub identity: Option<(String, String)>,
    /// The service its agent acts as, its bare ID (`LNK_AGENT_SERVICE`),
    /// when its settings name one.
    pub service: Option<String>,
    /// Link's immutable installation for this adapter version and agent.
    pub programs: Option<PathBuf>,
    /// Its sandbox policy's name, which names its rules file: the agent's
    /// and the harness's (`main-link`).
    pub profile: String,
    pub dir: PathBuf,
    /// The agent's two folders: Home, its files folder, and its own
    /// (`agent/`), its collections kept at `file`.
    pub folder: link_agent_contract::folder::Folder,
    /// For a harness that keeps to the agent's folder's layout (its
    /// contract's `folders`): the folders of what the agent sees it may
    /// write ([`link_agent_contract::folder::Folder::seen`]). It then reads Home,
    /// and its own folder's instructions and skills, and writes only
    /// these and its conversations; none: it writes the whole of Home,
    /// and never reaches its own folder.
    pub folders: Option<link_harness_contract::harness::Folders>,
    /// What it reaches of its teams' shared folders, beside its files
    /// folder, whichever harness runs it.
    pub teams: settings::TeamReach,
    pub sandbox: bool,
    /// The localhost ports it may connect to (its own, a local model's).
    pub ports: BTreeSet<u16>,
    /// Of `ports`, a local model's: behind a proxy, reached for inference
    /// only, never its admin API.
    pub models: BTreeSet<u16>,
    /// Its agent's base port (`LNK_PORT`); none before this machine gave
    /// it a block.
    pub port: Option<u16>,
    /// How many ports from `port` are its agent's.
    pub block: u16,
    /// Its programs' PATH.
    pub(super) path: String,
    /// Where a step's sockets are kept (`<name>-<step>` in it): its
    /// proxy's, or a health check's on the loopback.
    pub(super) net: PathBuf,
    /// Where each setup step's proxy keeps its sockets (`<name>-<step>`
    /// in it): with the sandbox and the internet, every step runs behind a
    /// proxy of its own, direct. None: as its grants say.
    pub(super) wires: Option<PathBuf>,
    /// What the agent may do while this harness runs it, as a sandbox's
    /// spec ([`settings::Settings::spec_of`]): as its grants say, and
    /// behind its proxy.
    pub(super) spec: Spec,
    pub(super) proxied: Spec,
}

impl Boxed {
    /// One of harness `h` for agent `main`, its folders not there, for
    /// tests.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        Self {
            name: "h".into(),
            agent: "main".into(),
            identity: None,
            service: None,
            programs: None,
            profile: "main-h".into(),
            folders: None,
            teams: Default::default(),
            port: None,
            block: settings::PORT_BLOCK,
            dir: "/nonexistent/home/h".into(),
            folder: link_agent_contract::folder::Folder {
                home: "/nonexistent/files".into(),
                agent: "/nonexistent/agent".into(),
            },
            sandbox: true,
            ports: BTreeSet::new(),
            models: BTreeSet::new(),
            path: String::new(),
            net: "/nonexistent/net".into(),
            wires: None,
            spec: Spec::default(),
            proxied: Spec::default(),
        }
    }

    /// One whose agent is granted `grants` (an agent's sandbox file), with
    /// `harness`'s grants (a spec's text), for tests.
    #[cfg(test)]
    pub(crate) fn granted(mut self, grants: &str, harness: &str) -> Self {
        // A fixture's text names its schema where it names none.
        let named = |text: &str| match text.lines().any(|line| line.starts_with("schema")) {
            true => text.to_string(),
            false => format!("schema = 1\n{text}"),
        };
        let file = crate::sandbox_file::Sandbox::parse(&named(grants)).unwrap();
        let harness: Spec = link_spec::parse(harness).unwrap();
        let here = |route| {
            file.resolve(&harness, &self.folder, route)
                .on(link_sandbox_contract::spec::Os::here())
                .unwrap()
        };
        self.spec = here(Route::Direct);
        self.proxied = here(Route::Proxied);
        self.ports.extend(numbered(&self.spec));
        self
    }

    /// The home of the agent's harness `name`, with the agent's files
    /// folder, sandbox and grants for it; created if needed.
    pub fn new(agent: &settings::Settings, name: &str) -> anyhow::Result<Self> {
        let dir = agent.box_of(name);
        let sandbox = agent.sandbox;
        // Without one (`--no-sandbox`), unless trusted machine policy
        // explicitly forbids it.
        if !sandbox {
            crate::machine_settings::check_unsandboxed()
                .within(format!("{} doesn't run {name} with no sandbox", agent.name))?;
        }
        settings::check_teams()?;
        let net = agent.wires();
        let spec = agent.spec_of(name, Route::Direct);
        let developer_tools = spec
            .commands
            .as_ref()
            .and_then(|c| c.developer_tools)
            .unwrap_or(false);
        // Granted the network, through its proxy or as it is: each setup
        // step behind a proxy of its own.
        let wires = (sandbox && spec.internet.is_some()).then(|| net.clone());
        // Only the user's: other users of this machine can't look in.
        link_files::create_private_dir(&dir.join("tmp"))
            .and_then(|()| link_files::create_private_dir(&dir))
            .within(format!("creating {}", dir.display()))?;
        let mut path = path(developer_tools);
        if cfg!(target_os = "macos") && sandbox {
            let shims = write_shims(&link_files::try_installation_dir()?)?;
            path = format!("{}:{path}", shims.display());
        }
        let mut boxed = Self {
            name: name.to_string(),
            agent: agent.name.clone(),
            identity: agent
                .agent_id()
                .map(str::to_string)
                .zip(agent.owner.clone()),
            service: agent.service.clone(),
            programs: None,
            profile: agent.profile(name),
            port: agent.port,
            block: agent.block(),
            dir,
            folder: agent.folder(),
            folders: None,
            teams: agent.team_reach(),
            sandbox,
            // The ports its spec names.
            ports: numbered(&spec).collect(),
            models: agent.model_port().into_iter().collect(),
            path,
            net,
            wires,
            spec,
            proxied: agent.spec_of(name, Route::Proxied),
        };
        // Its installed programs, where it has an adapter: a harness run
        // by name alone (`lnk agent harness run`) has none, and installs
        // nothing.
        boxed.programs = match Adapter::find(name) {
            Ok(adapter) => Some(programs::directory(&adapter, &boxed)?),
            Err(_) => None,
        };
        Ok(boxed)
    }

    /// The whole environment its programs get: Link's own.
    pub fn env(&self) -> BTreeMap<String, String> {
        let mut env = BTreeMap::from([
            ("PATH".into(), self.path.clone()),
            ("HOME".into(), self.dir.display().to_string()),
            ("TMPDIR".into(), self.dir.join("tmp").display().to_string()),
            ("LANG".into(), "en_US.UTF-8".into()),
            ("LNK_FILES".into(), self.folder.home.display().to_string()),
            (
                link_harness_contract::harness::AGENT_FOLDER_ENV.into(),
                self.folder.agent.display().to_string(),
            ),
            (AGENT_ENV.into(), self.agent.clone()),
        ]);
        if let Some(programs) = &self.programs {
            env.insert(
                link_files::layout::exec_variable().into(),
                programs.display().to_string(),
            );
        }
        if let Some(port) = self.port {
            env.insert(
                link_harness_contract::harness::PORT_ENV.into(),
                port.to_string(),
            );
        }
        if let Some((id, owner)) = &self.identity {
            env.insert(
                link_agent_contract::identity::AGENT_ID_ENV.into(),
                id.clone(),
            );
            env.insert(
                link_agent_contract::identity::OWNER_ENV.into(),
                owner.clone(),
            );
        }
        if let Some(service) = &self.service {
            env.insert(
                link_agent_contract::identity::SERVICE_ENV.into(),
                service.clone(),
            );
        }
        env.insert(
            link_measure_contract::client::ENV.into(),
            measures_of(&self.dir).display().to_string(),
        );
        // Unsandboxed, it reaches Link's settings, so it hosts its work in
        // the machine's interaction store, which its tools' sources check
        // ([`link_interactions::Host`]); no sandbox reaches them.
        if !self.sandbox
            && let Ok(store) = link_interactions::Store::machine()
        {
            env.insert(
                link_interactions::STORE_VAR.into(),
                store.dir().display().to_string(),
            );
        }
        env
    }

    /// `argv` wrapped in the sandbox (unless it's off), by the sandbox
    /// module (`lnk sandbox wrap`), with no proxy of its own
    /// ([`Boxed::unproxied`]), running what it writes in `runs`
    /// ([`run_written`]). `profile` names its rules file, which the
    /// sandbox keeps with Link's settings, where no harness can edit it.
    pub fn wrap(
        &self,
        argv: &[String],
        profile: &str,
        write: &[PathBuf],
        read: &[PathBuf],
        with_files: bool,
        runs: &[PathBuf],
    ) -> anyhow::Result<Vec<String>> {
        if !self.sandbox {
            return Ok(argv.to_vec());
        }
        let (spec, way) = self.unproxied(profile);
        let program = Path::new(argv.first().within("nothing to run")?);
        let mut confinement =
            self.confinement(spec, program, profile, write, read, with_files, way)?;
        run_written(&mut confinement, runs);
        Ok(link_sandbox_client::wrap(&confinement, argv)?.argv)
    }

    /// The agent's spec as a step of this harness runs it, behind a proxy
    /// of the step's own (`proxied`) or as its grants say: its localhost
    /// this harness's ports (a local model's only behind a proxy, reached
    /// for inference only), and of its files your home alone, if granted,
    /// each step adding its own ([`Boxed::confinement`]).
    pub fn spec(&self, proxied: bool) -> Spec {
        let mut spec = match proxied {
            true => self.proxied.clone(),
            false => self.spec.clone(),
        };
        let read_home = spec
            .files
            .as_ref()
            .and_then(|f| f.read.as_ref())
            .is_some_and(|read| read.iter().any(|p| p == "~"));
        // The dot files it's granted at the top of its files folder, which
        // the harness's confinement writes ([`Boxed::confinement`]).
        let dots = spec.files.as_ref().and_then(|f| f.write_dot_files.clone());
        spec.files = (read_home || dots.is_some()).then(|| Files {
            read: read_home.then(|| vec!["~".into()]),
            write_dot_files: dots,
            ..Files::default()
        });
        let every = spec
            .localhost
            .as_ref()
            .and_then(|l| l.ports.as_deref())
            .is_some_and(|ports| ports.iter().any(|p| matches!(p, Port::Word(_))));
        // A local model's port: behind a proxy, for inference only; with
        // none, which can't filter it, not at all.
        let models: Vec<u16> = match proxied {
            true => self.ports.intersection(&self.models).copied().collect(),
            false => Vec::new(),
        };
        let ports: Vec<Port> = match every {
            true => vec![Port::Word("*".into())],
            false => self
                .ports
                .iter()
                .filter(|p| !self.models.contains(p))
                .map(|p| Port::Number(*p))
                .collect(),
        };
        spec.localhost = (!ports.is_empty() || !models.is_empty()).then(|| Localhost {
            ports: (!ports.is_empty()).then_some(ports),
            models: (!models.is_empty()).then_some(models),
            ..Localhost::default()
        });
        spec
    }

    /// The agent's spec with no proxy of its own ([`Boxed::spec`]), and
    /// how its network is carried: the network as it is (`[network] mode =
    /// "host"`), or this
    /// machine's ports alone, relayed through `profile`'s folder of its
    /// sockets, or none.
    pub fn unproxied(&self, profile: &str) -> (Spec, Way) {
        let spec = self.spec(false);
        let way = match spec.network.is_some() {
            // Host: no proxy, whatever it would have been given.
            true => Way::Proxy(ProxyAt::default()),
            false => Way::Loopback(self.net.join(profile)),
        };
        (spec, way)
    }

    /// What it may touch, as `profile`, from `spec` (one of
    /// [`Boxed::spec`]'s) on `way`: its home and `write` (and, the harness
    /// itself, `with_files`, the files folder as it keeps it), `read`, its
    /// `/tmp/<name>*`, and `program`, which Link starts, with the programs
    /// `[commands]` grants it, and the developer tools if granted. The home comes first. Every other agent's folder is hidden,
    /// your home read or not, and what agents that left kept
    /// (`~/Link/Archive`), and every team's but what the harness itself is
    /// granted of its own.
    #[expect(
        clippy::too_many_arguments,
        reason = "each is a separate part of the confinement, chosen by the caller"
    )]
    pub fn confinement(
        &self,
        spec: Spec,
        program: &Path,
        profile: &str,
        write: &[PathBuf],
        read: &[PathBuf],
        with_files: bool,
        way: Way,
    ) -> anyhow::Result<Confinement> {
        // `[commands]` binds every program of the agent's, the harness's
        // as its `run`'s: what the harness starts beyond `program` it
        // names, in its needs or the agent's grants. A name is found as
        // `run` finds it, and one this machine lacks is left out. The
        // developer tools with no program (an empty `allow`) are granted
        // apart.
        let mut spec = spec.on(link_sandbox_contract::spec::Os::here())?;
        let developer_tools = spec.commands.as_ref().and_then(|c| c.developer_tools);
        spec.commands = spec.commands.take().and_then(|mut commands| {
            let allow: Vec<String> = commands
                .allow
                .take()
                .unwrap_or_default()
                .into_iter()
                .filter(|entry| {
                    entry.contains('/')
                        || link_harness_base::find_program(entry, crate::sandbox_file::PATH)
                            .is_some()
                })
                .collect();
            // Its toolchains whole: each names its programs, found here
            // as the sandbox finds them, and its builds' folders.
            let toolchains = commands.toolchains.take().unwrap_or_default();
            (!allow.is_empty() || !toolchains.is_empty()).then(|| {
                link_sandbox_contract::spec::Commands {
                    allow: (!allow.is_empty()).then_some(allow),
                    toolchains: (!toolchains.is_empty()).then_some(toolchains),
                    ..Default::default()
                }
            })
        });
        if with_files {
            let files = spec.files.get_or_insert_default();
            let (written, kept) = self.files();
            files.write.get_or_insert_default().extend(written);
            files.read.get_or_insert_default().extend(kept);
        } else if let Some(files) = &mut spec.files {
            // A step that isn't the harness writes no files folder, so
            // none of its dot files.
            files.write_dot_files = None;
            if files.read.is_none() && files.write.is_none() {
                spec.files = None;
            }
        }
        let home = link_files::home_dir();
        let mut confinement = Confinement::of(
            &spec,
            At {
                name: profile,
                dir: &self.dir,
                home: &home,
                path: Some(std::ffi::OsStr::new(crate::sandbox_file::PATH)),
                way,
            },
        )
        .within(format!("{}'s sandbox for {}", self.agent, self.name))?;
        if let link_sandbox_client::Programs::Only(programs) = &mut confinement.commands {
            // A name, found as its environment's PATH finds it.
            let program = match program.to_str().filter(|p| !p.contains('/')) {
                Some(name) => link_harness_base::find_program(name, &self.path)
                    .unwrap_or_else(|| program.to_path_buf()),
                None => program.to_path_buf(),
            };
            programs.extend(interpreter(&program));
            programs.push(program);
        }
        confinement.developer_tools = developer_tools.unwrap_or(false);
        // Its own folders are its own: nothing of yours runs there, and a
        // harness keeps a repository in them (a workspace it commits).
        // The files folder is yours, so its dot files that run outside
        // stay read only, unless its sandbox file grants them
        // (`[files] write_dot_files`), kept from its spec.
        let mut all_write = vec![self.dir.clone()];
        all_write.extend(write.iter().cloned());
        let granted = std::mem::take(&mut confinement.write_dot_files);
        confinement.write_dot_files = all_write.clone();
        confinement.write_dot_files.extend(granted);
        all_write.append(&mut confinement.write);
        let mut all_read = read.to_vec();
        if let Some(programs) = &self.programs
            && programs.is_dir()
        {
            all_read.push(programs.clone());
            if let link_sandbox_client::Programs::Only(allowed) = &mut confinement.commands {
                allowed.push(programs.clone());
            }
        }
        if cfg!(target_os = "macos") {
            let shim = link_files::layout::Hidden::Mktemp.path();
            all_read.push(shim.clone());
            if let link_sandbox_client::Programs::Only(programs) = &mut confinement.commands {
                programs.extend(interpreter(&shim));
                programs.push(shim);
            }
        }
        all_read.append(&mut confinement.read);
        if with_files {
            all_write.extend(self.teams.write.iter().cloned());
            all_read.extend(self.teams.read.iter().cloned());
        }
        confinement.write = all_write;
        confinement.read = all_read;
        confinement.hide = vec![settings::agents(), link_agent_contract::archive_dir()];
        confinement.hide.extend(self.teams.hide.iter().cloned());
        confinement.tmp_prefix = Some(self.name.clone());
        if matches!(&confinement.net, Net::Loopback { ports, .. } if ports.is_empty()) {
            confinement.net = Net::Closed;
        }
        Ok(confinement)
    }

    /// Of its files folder, what the harness itself writes and reads: the
    /// whole of it, or for a harness that keeps to the agent's folder's
    /// layout, Home and its own folder's instructions and skills read, and
    /// only its conversations and what the agent may write written
    /// ([`link_agent_contract::agent_sandbox::files`]): side by side, so a folder
    /// it only reads is never inside one it writes. Its notes' folder
    /// read too, `memory/`: its prompt holds the notes, which memory
    /// writes whole, a new file renamed over the old, so the folder, never
    /// the file, shows each.
    fn files(&self) -> (Vec<String>, Vec<String>) {
        let Some(folders) = &self.folders else {
            return (vec![self.folder.home.display().to_string()], Vec::new());
        };
        let files = link_agent_contract::agent_sandbox::files(folders, &self.folder);
        let conversations = self
            .folder
            .of(link_store_contract::storage::Collection::Conversations)
            .display()
            .to_string();
        let mut write = files.write.unwrap_or_default();
        let mut read: Vec<String> = files
            .read
            .unwrap_or_default()
            .into_iter()
            .filter(|p| *p != conversations)
            .collect();
        read.push(
            self.folder
                .of(link_store_contract::storage::Collection::Memory)
                .display()
                .to_string(),
        );
        if !write.contains(&conversations) {
            write.push(conversations);
        }
        (write, read)
    }
}

/// The ports `spec` names by number on this machine's loopback.
fn numbered(spec: &Spec) -> impl Iterator<Item = u16> + '_ {
    spec.localhost
        .iter()
        .flat_map(|l| l.ports.iter().flatten())
        .filter_map(|p| match p {
            Port::Number(n) => Some(*n),
            Port::Word(_) => None,
        })
}

/// The interpreter a script's `#!` line names by its path, which runs as
/// the script starts: none for a program that isn't one.
fn interpreter(program: &Path) -> Option<PathBuf> {
    use std::io::Read;
    let mut head = [0u8; 256];
    let n = std::fs::File::open(program)
        .and_then(|mut file| file.read(&mut head))
        .ok()?;
    let line = head[..n].strip_prefix(b"#!")?;
    let line = &line[..line.iter().position(|&b| b == b'\n').unwrap_or(line.len())];
    let first = std::str::from_utf8(line).ok()?.split_whitespace().next()?;
    first.starts_with('/').then(|| PathBuf::from(first))
}

/// A health check's confinement, reading `read` beside its home: the
/// harness's ports on this machine's loopback, never a local model's, and
/// no other way to a network, whatever the agent may do.
pub(super) fn health_confinement(
    boxed: &Boxed,
    program: &Path,
    read: &[PathBuf],
) -> anyhow::Result<Confinement> {
    let mut spec = boxed.spec(false);
    spec.network = None;
    spec.internet = None;
    spec.lan = None;
    spec.tools = None;
    spec.browser = None;
    let ports: Vec<Port> = boxed
        .ports
        .iter()
        .filter(|p| !boxed.models.contains(p))
        .map(|p| Port::Number(*p))
        .collect();
    spec.localhost = (!ports.is_empty()).then(|| Localhost {
        ports: Some(ports),
        ..Localhost::default()
    });
    boxed.confinement(
        spec,
        program,
        &format!("{}-setup", boxed.profile),
        &[],
        read,
        false,
        Way::Loopback(boxed.net.join(format!("{}-health", boxed.name))),
    )
}

/// `confinement` running what it writes in `runs`, folders of its `write`
/// (the install step's own folder, when its harness says its install runs
/// what it downloads): each a build folder of the sandbox's
/// ([`Confinement::builds`]), and in its commands. Nothing else it writes
/// runs.
pub(super) fn run_written(confinement: &mut Confinement, runs: &[PathBuf]) {
    for folder in runs {
        confinement.builds.push(folder.clone());
        if let link_sandbox_client::Programs::Only(programs) = &mut confinement.commands {
            programs.push(folder.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The program Link starts in each sandbox here: the harness's.
    fn h() -> &'static Path {
        Path::new("/opt/lnk-h")
    }

    /// A harness that keeps to the layout reads Home and its own folder's
    /// instructions and skills, and writes only its conversations and
    /// what the agent's sandbox file lets it; any other writes the whole
    /// of Home, and never reaches the agent's own folder.
    #[test]
    fn the_files_folder_is_written_as_the_sandbox_file_says() {
        let mut boxed = Boxed {
            name: "h".into(),
            agent: "main".into(),
            identity: None,
            service: None,
            programs: None,
            profile: "main-h".into(),
            folders: None,
            teams: Default::default(),
            port: None,
            block: settings::PORT_BLOCK,
            dir: "/nonexistent/home/h".into(),
            folder: link_agent_contract::folder::Folder {
                home: "/nonexistent/files".into(),
                agent: "/nonexistent/agent".into(),
            },
            sandbox: true,
            ports: BTreeSet::new(),
            models: BTreeSet::new(),
            path: String::new(),
            net: "/nonexistent/net".into(),
            wires: None,
            spec: Spec::default(),
            proxied: Spec::default(),
        };
        let files = PathBuf::from("/nonexistent/files");
        let own = PathBuf::from("/nonexistent/agent");
        let confined = |boxed: &Boxed, with_files: bool| {
            let (spec, way) = boxed.unproxied("p");
            boxed
                .confinement(spec, h(), "p", &[], &[], with_files, way)
                .unwrap()
        };
        let whole = confined(&boxed, true);
        assert!(whole.write.contains(&files));
        assert!(
            !whole
                .read
                .iter()
                .chain(&whole.write)
                .any(|p| p.starts_with(&own))
        );
        boxed.folders = Some(link_harness_contract::harness::Folders {
            write: vec!["work".into()],
            read: vec!["instructions".into(), "chats".into()],
            teams: Default::default(),
        });
        let kept = confined(&boxed, true);
        assert!(!kept.write.contains(&files));
        assert_eq!(
            kept.read,
            [
                files.clone(),
                own.join("instructions"),
                own.join("skills"),
                own.join("memory")
            ]
        );
        assert_eq!(kept.write[1..], [files.join("work"), own.join("chats")]);
        // Its instructions, granted by name beside the whole of Home.
        boxed.folders = Some(link_harness_contract::harness::Folders {
            write: vec![".".into(), "instructions".into()],
            read: Vec::new(),
            teams: Default::default(),
        });
        let both = confined(&boxed, true);
        assert_eq!(both.read, [own.join("skills"), own.join("memory")]);
        assert_eq!(
            both.write[1..],
            [files.clone(), own.join("instructions"), own.join("chats")]
        );
        // A step that isn't the harness touches none of it.
        let step = confined(&boxed, false);
        assert!(!step.read.contains(&files) && !step.write.iter().any(|w| w.starts_with(&files)));
        // Its own folder first, its dot files its own; any program;
        // Link's folders hidden; no network.
        assert_eq!(step.write, [PathBuf::from("/nonexistent/home/h")]);
        assert_eq!(step.write_dot_files, step.write);
        // Granted no `[commands]`, the program Link starts alone.
        assert_eq!(
            step.commands,
            link_sandbox_client::Programs::Only(vec![h().into()])
        );
        assert_eq!(
            step.hide,
            [settings::agents(), link_agent_contract::archive_dir()]
        );
        assert_eq!(step.tmp_prefix.as_deref(), Some("h"));
        assert_eq!(step.net, Net::Closed);
    }

    /// A step running what it writes in a folder runs it there alone: the
    /// folder a build folder of its sandbox and in its commands, and no
    /// other folder it writes, its home among them, either.
    #[test]
    fn a_step_runs_what_it_writes_only_where_it_is_told() {
        let boxed = Boxed::for_tests();
        let staged = PathBuf::from("/nonexistent/staging/abc");
        let confined = |runs: &[PathBuf]| {
            let (spec, way) = boxed.unproxied("p");
            let mut confinement = boxed
                .confinement(
                    spec,
                    h(),
                    "p",
                    std::slice::from_ref(&staged),
                    &[],
                    false,
                    way,
                )
                .unwrap();
            run_written(&mut confinement, runs);
            confinement
        };
        let listed = |c: &Confinement| c.commands.clone().listed().unwrap_or_default();
        let plain = confined(&[]);
        assert!(plain.write.contains(&staged));
        assert!(plain.builds.is_empty());
        assert!(!listed(&plain).contains(&staged));
        let installing = confined(std::slice::from_ref(&staged));
        assert_eq!(installing.builds, std::slice::from_ref(&staged));
        assert!(listed(&installing).contains(&staged));
        for written in installing.write.iter().filter(|w| **w != staged) {
            assert!(
                !listed(&installing).contains(written),
                "{}",
                written.display()
            );
        }
    }

    /// A coding agent granted its repository's dot files (`[files]
    /// write_dot_files`) writes them in its files folder, so it commits;
    /// without the grant only its own folder's are its own.
    #[test]
    fn dot_files_granted_reach_its_confinement() {
        let files = PathBuf::from("/nonexistent/files");
        let confined = |boxed: &Boxed| {
            let (spec, way) = boxed.unproxied("p");
            boxed
                .confinement(spec, h(), "p", &[], &[], true, way)
                .unwrap()
        };
        let plain = Boxed::for_tests().granted("[files]\nwrite = [\".\"]\n", "");
        assert!(!confined(&plain).write_dot_files.contains(&files));
        let granted =
            Boxed::for_tests().granted("[files]\nwrite = [\".\"]\nwrite_dot_files = [\".\"]\n", "");
        let confinement = confined(&granted);
        assert!(
            confinement.write_dot_files.contains(&files),
            "{:?}",
            confinement.write_dot_files
        );
        assert!(
            confinement
                .write_dot_files
                .contains(&PathBuf::from("/nonexistent/home/h"))
        );
    }

    /// Its notes are in its prompt from the next message: the harness
    /// reads the folder memory writes them to, whichever folders it writes.
    #[test]
    fn the_harness_reads_the_notes_memory_writes() {
        let notes = std::path::Path::new("/nonexistent/agent")
            .join(link_harness_contract::harness::layout::NOTES);
        for write in [
            vec!["work".to_string()],
            vec![".".into(), "instructions".into()],
        ] {
            let mut boxed = Boxed::for_tests();
            boxed.folders = Some(link_harness_contract::harness::Folders {
                write,
                read: Vec::new(),
                teams: Default::default(),
            });
            let (spec, way) = boxed.unproxied("p");
            let confinement = boxed
                .confinement(spec, h(), "p", &[], &[], true, way)
                .unwrap();
            assert!(
                confinement
                    .read
                    .iter()
                    .any(|p| Some(p.as_path()) == notes.parent()),
                "{:?}",
                confinement.read
            );
        }
    }

    /// An agent working in a repository: its own sandbox file, its files folder
    /// the repository. The harness writes the whole folder and reaches
    /// the service's port, and no other, as the file says.
    #[test]
    fn an_agent_in_a_repository_is_sandboxed_as_its_file_says() {
        let sandbox = crate::sandbox_file::Sandbox::parse(
            "schema = 1\n[files]\nwrite = [\".\"]\n[internet]\nhosts = [\"pypi.org\"]\n\
             [localhost]\nports = [5432]\n[commands]\nallow = [\"sh\"]\n",
        )
        .unwrap();
        let files = PathBuf::from("/nonexistent/u/app");
        let text = sandbox.text("main").unwrap();
        let boxed = Boxed {
            name: "link".into(),
            agent: "main".into(),
            identity: None,
            service: None,
            programs: None,
            profile: "main-link".into(),
            folders: Some(sandbox.folders()),
            teams: Default::default(),
            port: None,
            block: settings::PORT_BLOCK,
            dir: "/nonexistent/home/link".into(),
            folder: link_agent_contract::folder::Folder {
                home: files.clone(),
                agent: "/nonexistent/agent".into(),
            },
            sandbox: true,
            ports: BTreeSet::new(),
            models: BTreeSet::new(),
            path: String::new(),
            net: "/nonexistent/net".into(),
            wires: None,
            spec: Spec::default(),
            proxied: Spec::default(),
        };
        let boxed = boxed.granted(&text, "");
        let at = ProxyAt {
            upstream: "direct".into(),
            dir: "/nonexistent/net/link".into(),
            ..ProxyAt::default()
        };
        let confinement = boxed
            .confinement(boxed.spec(true), h(), "p", &[], &[], true, Way::Proxy(at))
            .unwrap();
        assert!(
            confinement.write.contains(&files),
            "{:?}",
            confinement.write
        );
        assert!(!confinement.read.contains(&files));
        let Net::Proxied { proxy, ports } = &confinement.net else {
            panic!("{:?}", confinement.net)
        };
        assert_eq!(*ports, BTreeSet::from([5432]));
        assert!(proxy.pages().is_none() && !proxy.no_internet);
        assert_eq!(proxy.hosts, ["pypi.org"]);
        assert_eq!(sandbox.spec.internet.unwrap().hosts.unwrap(), ["pypi.org"]);
        // Its `[commands]` bind the harness as they bind its `run`: the
        // program Link starts, and those they name.
        let sh = link_harness_base::find_program("sh", crate::sandbox_file::PATH)
            .unwrap()
            .canonicalize()
            .unwrap();
        assert_eq!(
            confinement.commands,
            link_sandbox_client::Programs::Only(vec![sh, h().into()])
        );
    }

    #[test]
    fn a_health_check_reaches_the_harness_on_the_loopback_alone() {
        let mut boxed = Boxed::for_tests().granted(
            "[network]\nmode = \"host\"\n[internet]\nhosts = [\"*\"]\n[localhost]\nports = \
             [\"*\"]\n[files]\nread = [\"~\"]\n[commands.macos]\ndeveloper_tools = true\n",
            "",
        );
        boxed.port = Some(20000);
        boxed.ports = BTreeSet::from([20000, 11434]);
        boxed.models = BTreeSet::from([11434]);
        boxed.wires = Some("/nonexistent/net".into());
        let health = health_confinement(&boxed, h(), &[PathBuf::from("/opt/lnk-h")]).unwrap();
        // Its own port, not the model's, and nothing that opens a network.
        assert_eq!(
            health.net,
            Net::Loopback {
                ports: BTreeSet::from([20000]),
                dir: PathBuf::from("/nonexistent/net/h-health")
            }
        );
        assert_eq!(health.read, [PathBuf::from("/opt/lnk-h")]);
        assert!(health.read_home);
        assert_eq!(health.developer_tools, cfg!(target_os = "macos"));
    }

    /// Each way the harness runs, its network: behind its proxy, its
    /// ports and a local model's for inference only; on the network as it is,
    /// the network
    /// as it is, every port with localhost; without network, its ports
    /// alone, on the loopback.
    #[test]
    fn its_network_is_one_mode_for_each_way_it_runs() {
        let mut boxed = Boxed::for_tests();
        boxed.ports = BTreeSet::from([20000, 11434]);
        boxed.models = BTreeSet::from([11434]);
        let at = || ProxyAt {
            upstream: "direct".into(),
            dir: "/nonexistent/net/h".into(),
            ..ProxyAt::default()
        };
        let proxied = |boxed: &Boxed| {
            boxed
                .confinement(boxed.spec(true), h(), "p", &[], &[], true, Way::Proxy(at()))
                .unwrap()
                .net
        };
        let unproxied = |boxed: &Boxed| {
            let (spec, way) = boxed.unproxied("p");
            boxed
                .confinement(spec, h(), "p", &[], &[], true, way)
                .unwrap()
                .net
        };
        let network = boxed.granted(
            "[internet]\nhosts = [\"*\"]\n[browser]\nhosts = [\"*\"]\n",
            "",
        );
        let Net::Proxied { proxy, ports } = proxied(&network) else {
            panic!()
        };
        assert_eq!(ports, BTreeSet::from([20000]));
        assert_eq!(proxy.models(), BTreeSet::from([11434]));
        assert!(proxy.pages().unwrap() == ["*"] && !proxy.no_internet);
        let mut lan = Boxed::for_tests().granted(
            "[network]\nmode = \"host\"\n[internet]\nhosts = [\"*\"]\n[localhost]\nports = \
             [\"*\"]\n",
            "",
        );
        lan.ports = BTreeSet::from([20000, 11434]);
        assert_eq!(
            unproxied(&lan),
            Net::Host {
                ports: BTreeSet::new(),
                localhost: true
            }
        );
        // Routed through an exit, the network as it is reaches nothing past
        // its proxy.
        assert!(matches!(proxied(&lan), Net::Proxied { proxy, .. } if proxy.lan.is_empty()));
        let mut offline = Boxed::for_tests().granted("", "");
        offline.ports = BTreeSet::from([20000, 11434]);
        offline.models = BTreeSet::from([11434]);
        // Without a proxy, a model's port isn't reached: nothing would
        // keep it to inference.
        assert_eq!(
            unproxied(&offline),
            Net::Loopback {
                ports: BTreeSet::from([20000]),
                dir: "/nonexistent/net/p".into()
            }
        );
        // Routed, it reaches no host it isn't granted: behind its proxy
        // for its model alone.
        assert!(matches!(proxied(&offline), Net::Proxied { proxy, .. } if proxy.no_internet));
        let nothing = Boxed::for_tests().granted("", "");
        assert_eq!(unproxied(&nothing), Net::Closed);
    }
}
