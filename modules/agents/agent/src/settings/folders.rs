//! Where an agent's things are: its folders and logs, what its sandbox
//! spec reaches, its files folder, and its connections.

use super::*;
use link_format::WithinExt;

impl Settings {
    /// `~/Link/Agents/<name>`: everything of this agent.
    pub fn dir(&self) -> PathBuf {
        agent_dir(&self.name)
    }

    /// `~/Link/Agents/<name>/home`: its default files folder.
    pub fn home(&self) -> PathBuf {
        home(&self.name)
    }

    /// One of its own folders in `~/.lnk/data/agents/<name>/`: what it
    /// keeps while it runs that isn't its state.
    pub fn data(&self, which: link_files::layout::AgentData) -> PathBuf {
        which.at(&link_files::installation_dir(), &self.name)
    }

    /// `~/.lnk/data/agents/<name>/harnesses`: one home per harness.
    pub fn boxes(&self) -> PathBuf {
        boxes(&self.name)
    }

    /// The home of its harness `harness`.
    pub fn box_of(&self, harness: &str) -> PathBuf {
        self.boxes().join(harness)
    }

    /// The name of harness `harness`'s sandbox policy, which names its
    /// rules file: `<agent>-<harness>`.
    pub fn profile(&self, harness: &str) -> String {
        format!("{}-{harness}", self.name)
    }

    /// Where harness `harness`'s proxy sockets are (the sandbox's
    /// filtering proxy): with Link's settings, out of every harness's
    /// reach but this one's.
    pub fn net_dir(&self, harness: &str) -> PathBuf {
        self.wires().join(harness)
    }

    /// The folder of its sandboxes' sockets' folders
    /// (`<runtime>/lnk/agent/network/<agent id>`): keyed by its ID, which no
    /// other owner's agent shares.
    pub fn wires(&self) -> PathBuf {
        link_files::layout::Runtime::AgentNetwork
            .path()
            .join(self.key())
    }

    /// What its services, tools and sockets are keyed by: its ID, which
    /// no other owner's agent has, never its name, which every owner has
    /// a `main` of; its name only before it is first saved and has one.
    pub fn key(&self) -> String {
        self.agent_id()
            .map_or_else(|| self.name.clone(), str::to_string)
    }

    /// Its ID, bare, once it has one: `id`'s after its kind.
    pub fn agent_id(&self) -> Option<&str> {
        self.id.as_ref().map(link_id::Workload::id)
    }

    /// It as a secret's grant names it: its ID, its name and the service
    /// it acts as. An error before its first save, which mints them.
    pub fn secret_agent(&self) -> anyhow::Result<link_secret_contract::Grantee> {
        let id = self.id.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "agent {} has no ID yet: its first save makes one",
                self.name
            )
        })?;
        link_secret_contract::Grantee::new(id.as_str(), &self.name, self.service_id()?)
    }

    /// The service it acts as, its ID: who its secrets are given to. An
    /// error before its first save, which mints it.
    pub fn service_id(&self) -> anyhow::Result<&str> {
        self.service.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "agent {} acts as no service yet: its first save makes one",
                self.name
            )
        })
    }

    /// The log of harness `harness`, with Link's settings: outside its
    /// home, so the harness can't swap it for a link to a file of the
    /// user's, which launchd or systemd would then write its output into.
    pub fn log_path(&self, harness: &str) -> PathBuf {
        config_of(&self.name)
            .join("logs")
            .join(format!("{harness}.log"))
    }

    /// What this agent may do while harness `name` runs it, as a
    /// sandbox's spec: what you grant the agent
    /// (`~/Link/Safe/Agents/<agent>/sandbox.toml`) and what `name` said
    /// it needs and you allowed (`~/.lnk/safe/agents/harness-grants/<name>.toml`),
    /// laid out on this machine ([`Sandbox::resolve`]). On `route`: as
    /// they say, or routed, as through an exit, always behind its proxy.
    pub fn spec_of(&self, name: &str, route: Route) -> link_sandbox_client::Spec {
        let spec = crate::sandbox_file::of(&self.name).resolve(
            &Grants::of(name).spec,
            &self.folder(),
            route,
        );
        spec.on(link_sandbox_contract::spec::Os::here())
            .unwrap_or_default()
    }

    /// The only hosts its harness may reach through its proxy; empty,
    /// any.
    pub fn hosts_of(&self, name: &str) -> Vec<String> {
        let spec = self.spec_of(name, Route::Direct);
        let hosts = spec.internet.and_then(|i| i.hosts).unwrap_or_default();
        match hosts.iter().any(|h| h == "*") {
            true => Vec::new(),
            false => hosts,
        }
    }

    /// The tools its harness may call through its proxy.
    pub fn tools_of(&self, name: &str) -> Vec<String> {
        self.spec_of(name, Route::Direct)
            .tools
            .and_then(|t| t.names)
            .unwrap_or_default()
    }

    /// The tool its memory is served as (`memory-<agent id>`), whichever
    /// harness runs it, when it has one.
    pub fn memory_tool(&self) -> Option<String> {
        self.memory.as_ref().map(|_| memory_tool_of(&self.key()))
    }

    /// Where memory engine `engine` keeps what it keeps for this agent
    /// (`~/Link/Agents/<name>/agent/memory/<engine>`): in its own folder,
    /// which no harness's sandbox reaches.
    pub fn memory_state(&self, engine: &str) -> PathBuf {
        self.folder().index(engine)
    }

    /// Its two folders here: Home, its files folder, and its own
    /// (`~/Link/Agents/<name>/agent`), its collections kept at `file`.
    pub fn folder(&self) -> link_agent_contract::folder::Folder {
        link_agent_contract::folder::Folder::new(&self.dir(), Some(&self.files()))
    }

    /// The upstream of harness `name`'s proxy, when it has one: sandboxed
    /// but not on `[network] mode = "host"`, its only network, `direct`
    /// unless routed elsewhere, reaching what its `[internet]` grants or
    /// no host at all; routed (an exit), always; with no sandbox,
    /// `direct`, beside it, advice it may ignore (an exit refused as it
    /// starts). None: sandboxed on the network as it is.
    pub fn proxy_of(&self, name: &str) -> Option<String> {
        if let Some(upstream) = &self.upstream {
            return Some(upstream.clone());
        }
        // A sandboxed harness is behind its proxy whatever it reaches: the
        // proxy serves the harness's own port and carries Link's ways in,
        // with no internet as with some. Only the network as it is has
        // none. With no wall, a proxy for what doesn't need one (its
        // tools, the log, `stop`).
        let proxied = !self.sandbox || !self.spec_of(name, Route::Direct).host_network();
        proxied.then(|| "direct".to_string())
    }

    /// The harness it runs.
    pub fn harness(&self) -> &str {
        self.harness.as_deref().unwrap_or(DEFAULT_HARNESS)
    }

    /// The connections connected (`lnk agent connect`), by name.
    pub fn connections(&self) -> Vec<String> {
        self.connections.keys().cloned().collect()
    }

    /// Connection `connection`'s name as people write it (its module's title),
    /// or its name; either with nothing a terminal would act on.
    pub fn title(&self, connection: &str) -> String {
        let title = self
            .connections
            .get(connection)
            .map(|c| c.about.title.as_str())
            .filter(|t| !t.is_empty())
            .unwrap_or(connection);
        link_format::clean(title)
    }

    /// Keeps connection `connection` as its module made it (`lnk-<connection>
    /// connect`) and describes it: its settings, and its bot.
    pub fn connect(
        &mut self,
        connection: &str,
        connected: link_connection_contract::Connected,
        about: link_connection_contract::Describe,
    ) -> anyhow::Result<()> {
        crate::connections::check_name(connection)?;
        anyhow::ensure!(
            connected.settings.expose().is_object(),
            "lnk-{connection} connect made no settings"
        );
        // A secret no longer kept stays nowhere.
        self.disconnect(connection);
        self.connections
            .insert(connection.to_string(), Connection::new(connected, about));
        Ok(())
    }

    /// The id of its bot on `connection`, if it's connected there: what
    /// another agent's `connect` refuses.
    pub fn bot_id(&self, connection: &str) -> Option<String> {
        self.connections
            .get(connection)
            .map(|c| c.bot.id.clone())
            .filter(|id| !id.is_empty())
    }

    /// Forgets connection `connection`: its settings, and, as it's saved, its
    /// keys in your secrets; whether it was connected.
    pub fn disconnect(&mut self, connection: &str) -> bool {
        self.connections.remove(connection).is_some()
    }

    pub fn files(&self) -> PathBuf {
        self.files.clone().unwrap_or_else(|| self.home())
    }

    /// The files folder, if it's one [`Settings::check_files`] would accept
    /// as it is (for other modules: an old or hand-edited setting is left
    /// out rather than reported).
    pub fn safe_files(&self) -> Option<PathBuf> {
        let files = self.files();
        let boxes = self.boxes();
        let old = files == root() || files == root().join("Files");
        let clash = boxes.starts_with(&files)
            || files.starts_with(&boxes)
            || self.others_folder(&files).is_some();
        (!old && !clash && unsafe_files(&files).is_none()).then_some(files)
    }

    /// Makes sure the files folder doesn't hold the harnesses' homes (every
    /// harness writes the files folder, so it could reach another's home) and
    /// isn't inside one. The old defaults move to the agent's Home:
    /// `~/Link` itself (the setting only) and `~/Link/Files` (the folder
    /// too, unless Home exists); returns a note saying so.
    pub fn check_files(&mut self) -> anyhow::Result<Option<String>> {
        let files = self.files();
        let boxes = self.boxes();
        let home = self.home();
        if files == root() {
            self.files = Some(home.clone());
            return Ok(Some(format!(
                "Your agent's files folder is now {} (~/Link also holds each harness's own folder, \
                 which other harnesses must not reach). Move anything you want it to see there.",
                home.display()
            )));
        }
        if files == root().join("Files") && !home.exists() {
            if files.exists() {
                std::fs::create_dir_all(self.dir())?;
                std::fs::rename(&files, &home).within(format!(
                    "moving {} to {}",
                    files.display(),
                    home.display()
                ))?;
            }
            self.files = Some(home.clone());
            return Ok(Some(format!(
                "Your agent's files moved from {} to {}.",
                files.display(),
                home.display()
            )));
        }
        if boxes.starts_with(&files) || files.starts_with(&boxes) {
            bail!(
                "the files folder can't contain or be inside {} (the harnesses' own folders); \
                 pick another with --run.files",
                boxes.display()
            );
        }
        if let Some(why) = self.others_folder(&files) {
            bail!(
                "the files folder can't be {}: {why}; pick another with --run.files",
                files.display()
            );
        }
        if let Some(why) = unsafe_files(&files) {
            bail!(
                "the files folder can't be {}: {why}; pick another with --run.files",
                files.display()
            );
        }
        Ok(None)
    }

    /// Why `files` would reach other agents' folders, or what Link keeps
    /// of this one's, if it would: it holds `~/Link/Agents`, is in there
    /// but not in this agent's own, or is in its own but not its Home
    /// (its own folder, `agent/`, or its harnesses').
    pub(super) fn others_folder(&self, files: &Path) -> Option<&'static str> {
        let agents = agents();
        if agents.starts_with(files) {
            return Some("it holds every agent's folder");
        }
        if files.starts_with(self.dir()) {
            return (!files.starts_with(self.home()))
                .then_some("it's in the agent's folder, outside its Home, where Link keeps the agent's own folder and its harnesses");
        }
        files
            .starts_with(&agents)
            .then_some("it's in another agent's folder")
    }
}
