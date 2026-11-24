//! An agent's sandbox file (`~/Link/Safe/Agents/<agent>/sandbox.toml`):
//! what you grant the agent, whichever harness runs it, as a sandbox's
//! [`Spec`] in the agent's own terms, with the folders of its teams' it
//! reaches beside it.
//!
//! Its `[files]` names what the agent sees: a folder of Home (`work`), or
//! of its own folder's instructions, skills or conversations, by name;
//! `.` for all of Home, and in `read`, `"~"` for your whole home. Home
//! itself it always reads. Laid out on a machine
//! ([`AgentSandbox::resolve`]), those are folders there, so the file
//! means the same after a move or a rename. Its `[teams.<team>]` names
//! the folders of a team's shared folder it writes and reads, which only
//! the harness knows where to find.

use link_format::WithinExt;
use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{bail, ensure};
use serde::{Deserialize, Serialize};

use crate::folder::Folder;
use link_harness_contract::harness::{Commands, Folders, WHOLE};
use link_sandbox_contract::spec::{self, NetworkMode, Os, Port, Spec};

/// What you grant one agent: a sandbox spec, its teams' folders its
/// `[teams]`, held to what an agent's file can say ([`AgentSandbox::check`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Spec", into = "Spec")]
pub struct AgentSandbox {
    pub spec: Spec,
}

impl From<AgentSandbox> for Spec {
    fn from(sandbox: AgentSandbox) -> Spec {
        sandbox.spec
    }
}

/// How an agent's traffic leaves this machine, which its spec alone
/// doesn't say: as it says, or routed, through an exit or a machine's
/// default route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Direct,
    /// Always behind its proxy, to the internet through the route: the
    /// hosts its spec grants, and every host only where it grants
    /// `[network] mode = "host"`; `[lan]` reaches nothing past it.
    Proxied,
}

/// The beginnings of the tools Link defines for each agent alone: its
/// memory (`memory-<id>`), its scheduler (`scheduler-<id>`), its `run`
/// and its `file` (`run-<id>`, `file-<id>`), handed to that agent's
/// harness by Link, never granted by name.
pub const LINKS_OWN_TOOLS: [&str; 4] = ["memory-", "scheduler-", "run-", "file-"];

/// A tool's name an agent may be granted, or why not: one of Link's own
/// for an agent ([`LINKS_OWN_TOOLS`]) or a team's memory
/// (`team-<id>-memory`) is another agent's or team's, or its own
/// already.
pub fn check_tool(tool: &str) -> anyhow::Result<()> {
    link_sandbox_client::routes::check_tool_name(tool)?;
    ensure!(
        !LINKS_OWN_TOOLS.iter().any(|own| tool.starts_with(own))
            && !(tool.starts_with("team-") && tool.ends_with("-memory")),
        "{tool} is an agent's own memory, scheduler, run or file, or a team's memory, which Link \
         hands only to that agent or the team's members: it can't be granted"
    );
    ensure!(
        !link_harness_contract::harness::NAMESPACES.contains(&tool),
        "{tool} is the name of one of Link's namespaces of tools: a tool can't be granted it"
    );
    Ok(())
}

impl TryFrom<Spec> for AgentSandbox {
    type Error = anyhow::Error;

    /// A file, or a move's manifest's, strictly: what an agent's file can
    /// say of the spec ([`AgentSandbox::check`]).
    fn try_from(spec: Spec) -> anyhow::Result<Self> {
        let sandbox = Self { spec };
        sandbox.check()?;
        Ok(sandbox)
    }
}

impl AgentSandbox {
    /// Link's optional template, never a missing-file fallback
    /// ([`link_agent_spec::LINK_SANDBOX`]).
    pub fn link() -> Self {
        static LINKS: std::sync::LazyLock<AgentSandbox> = std::sync::LazyLock::new(|| {
            AgentSandbox::parse(link_agent_spec::LINK_SANDBOX).expect("Link's sandbox file reads")
        });
        LINKS.clone()
    }

    /// Nothing at all: no folder of its files written, no network, no
    /// program run.
    pub fn none() -> Self {
        Self::default()
    }

    /// Reads `text` strictly ([`AgentSandbox::read`]), as a spec's file
    /// is read: no empty table.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        Self::try_from(link_spec::parse_spec::<Spec>(text)?)
    }

    /// Reads `table` strictly: an unknown key, or a host, endpoint, port,
    /// tool, program or folder its file can't hold, is refused.
    pub fn read(table: toml::Table) -> anyhow::Result<Self> {
        Self::try_from(Spec::deserialize(table)?)
    }

    /// An error for what its file can't hold: a folder that isn't one
    /// the agent sees, a host, endpoint, port, tool or program that isn't
    /// one, `"*"` beside other items, a model's port (its model is its
    /// own), or a team's name or folders that aren't.
    pub fn check(&self) -> anyhow::Result<()> {
        self.spec.check_any_alone()?;
        if let Some(files) = &self.spec.files {
            ensure!(
                files.hide.is_none(),
                "[files]: an agent's names the folders it writes and reads, and only those"
            );
            // The dot files that run outside the sandbox (`.git` and the
            // rest) written, only at the top of a folder it writes: a
            // grant you give knowing what runs from them later.
            for dots in files.write_dot_files.iter().flatten() {
                ensure!(
                    files.write.iter().flatten().any(|w| w == dots),
                    "[files] write_dot_files {dots:?}: only a folder it writes, named in write"
                );
            }
            ensure!(
                files.macos.is_none() && files.linux.is_none(),
                "[files]: an agent's folders are the same on every OS"
            );
        }
        Folders {
            teams: BTreeMap::new(),
            ..self.folders()
        }
        .check()?;
        for (team, folders) in &self.spec.teams {
            link_machine::machine::check_name(team).within(format!("[teams.{team}]"))?;
            folders.check(&format!("[teams.{team}]"))?;
        }
        ensure!(
            self.spec
                .files
                .iter()
                .flat_map(|f| f.write.iter().flatten())
                .all(|w| w != "~"),
            "[files] write \"~\": an agent reads your home, never writes it whole"
        );
        for os in [Os::Linux, Os::MacOs] {
            let spec = self.spec.on(os)?;
            if let Some(internet) = &spec.internet {
                ensure!(internet.hosts.is_some(), "[internet] needs hosts");
                for host in internet.hosts.iter().flatten().filter(|h| *h != "*") {
                    link_sandbox_contract::net::check_host(host)?;
                }
                for endpoint in internet.tcp.iter().flatten() {
                    link_sandbox_contract::net::check_endpoint(endpoint)?;
                }
                ensure!(
                    internet.per_minute != Some(0),
                    "[internet] per_minute is at least 1"
                );
            }
            for host in spec.lan.iter().flat_map(|l| l.hosts.iter().flatten()) {
                link_sandbox_contract::net::check_lan(host)?;
            }
            if let Some(localhost) = &spec.localhost {
                ensure!(
                    localhost.models.as_ref().is_none_or(Vec::is_empty),
                    "[localhost] models: an agent's model is its own, set by lnk agent start"
                );
                let host = spec.network.as_ref().and_then(|n| n.mode) == Some(NetworkMode::Host);
                for port in localhost.ports.iter().flatten() {
                    match port {
                        Port::Number(p) if *p != 0 && *p != link_sandbox_client::PROXY_PORT => {}
                        Port::Word(t) if t == "*" && host => {}
                        other => bail!(
                            "[localhost] ports {other:?}: an agent reaches the ports it names, \
                             each a number but Link's proxy's ({}); every port, \"*\", only with \
                             [network] mode = \"host\"",
                            link_sandbox_client::PROXY_PORT
                        ),
                    }
                }
            }
            for tool in spec.tools.iter().flat_map(|t| t.names.iter().flatten()) {
                check_tool(tool)?;
            }
            if let Some(commands) = &spec.commands {
                Commands {
                    allow: commands.allow.clone().unwrap_or_default(),
                    toolchains: commands.toolchains.clone().unwrap_or_default(),
                }
                .check()?;
            }
        }
        Ok(())
    }

    /// Its `[files]` as the folders of what the agent sees it writes and
    /// only reads, `"~"` left out; its teams' beside them.
    pub fn folders(&self) -> Folders {
        let files = self.spec.files.clone().unwrap_or_default();
        Folders {
            write: files.write.unwrap_or_default(),
            read: files
                .read
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r != "~")
                .collect(),
            teams: self.spec.teams.clone(),
        }
    }

    /// The folders it writes whose dot files that run outside a sandbox
    /// (`.git`, `.envrc`) it may write too (`[files] write_dot_files`).
    pub fn write_dot_files(&self) -> Vec<String> {
        self.spec
            .files
            .as_ref()
            .and_then(|f| f.write_dot_files.clone())
            .unwrap_or_default()
    }

    /// Its `[commands]` as its `run`'s programs here; none, no `run`.
    pub fn commands(&self) -> Option<Commands> {
        let commands = self.spec.on(Os::here()).ok()?.commands?;
        Some(Commands {
            allow: commands.allow.unwrap_or_default(),
            toolchains: commands.toolchains.unwrap_or_default(),
        })
    }

    /// Whether its programs run with the developer tools here
    /// (`[commands.macos] developer_tools`, Xcode's): its `run`'s and its
    /// `code`'s servers as its harness's, since `/usr/bin/git` and
    /// `/usr/bin/clangd` run the real ones through `xcrun`.
    pub fn developer_tools(&self) -> bool {
        self.spec
            .on(Os::here())
            .ok()
            .and_then(|spec| spec.commands)
            .and_then(|commands| commands.developer_tools)
            .unwrap_or(false)
    }

    /// Whether it reads your whole home (`[files] read = ["~"]`).
    pub fn reads_home(&self) -> bool {
        self.spec
            .files
            .iter()
            .flat_map(|f| f.read.iter().flatten())
            .any(|r| r == "~")
    }

    /// The file, as kept: what it grants, and how to change it.
    pub fn text(&self, agent: &str) -> anyhow::Result<String> {
        Ok(format!(
            "# What {agent} may do on this machine, whichever harness runs it: a sandbox's spec,\n\
             # its [files] the folders it sees (work, . for all of Home, ~ your home).\n\
             # lnk agent show --permissions shows it; lnk agent permission add and deny change it.\n\n{}",
            link_spec::print_spec(&self.spec)?
        ))
    }

    /// The spec it runs with, with `harness`'s (what its harness asked for
    /// and was allowed) beside it, its files folder laid out as `folder`,
    /// on `route`: Home read, or written with `.`; each folder it names
    /// written or read; its own folder's instructions, skills and
    /// conversations it doesn't name read
    /// ([`files`]). Its teams' are its harness's to
    /// add, as only the harness knows where they are.
    pub fn resolve(&self, harness: &Spec, folder: &Folder, route: Route) -> Spec {
        // Behind its proxy each side keeps only what the proxy carries,
        // before they're joined: a harness's port stays beside the
        // agent's every port (`"*"`), which only the network as it is
        // reaches.
        let routed = |spec: &Spec| {
            let mut spec = spec.clone();
            if route == Route::Proxied {
                spec.network = None;
                spec.lan = None;
                if let Some(localhost) = &mut spec.localhost
                    && let Some(ports) = &mut localhost.ports
                {
                    ports.retain(|p| matches!(p, Port::Number(_)));
                    if ports.is_empty() {
                        localhost.ports = None;
                    }
                }
                if spec
                    .localhost
                    .as_ref()
                    .is_some_and(|l| l.ports.is_none() && l.models.is_none())
                {
                    spec.localhost = None;
                }
            }
            spec
        };
        // The whole network as it is, granted, is every host behind the
        // proxy; nothing else adds one, so a routed spec never reaches a
        // host the same spec on the network as it is doesn't.
        let whole = [&self.spec, harness].into_iter().any(|spec| {
            spec.network
                .as_ref()
                .is_some_and(|n| n.mode == Some(NetworkMode::Host))
        });
        let mut spec = routed(&self.spec).join(&routed(harness));
        // Its teams' folders are its harness's to lay out: never a run's.
        spec.teams.clear();
        let mut files = files(&self.folders(), folder);
        let mut more = harness.files.clone().unwrap_or_default();
        if self.reads_home() {
            files.read.get_or_insert_default().push("~".into());
        }
        // A harness's folders are as it named them: absolute, or `~`.
        files.write = spec::union(&files.write, &more.write.take());
        files.read = spec::union(&files.read, &more.read.take());
        // The dot files it writes at the top of folders it writes, laid
        // out as those are; a harness's as it named them.
        let dots: Vec<String> = self
            .spec
            .files
            .iter()
            .flat_map(|f| f.write_dot_files.iter().flatten())
            .map(|name| match name == WHOLE {
                true => folder.home.display().to_string(),
                false => folder.seen(name).display().to_string(),
            })
            .collect();
        files.write_dot_files = spec::union(
            &(!dots.is_empty()).then_some(dots),
            &more.write_dot_files.take(),
        );
        files.hide = more.hide.take();
        spec.files = Some(files);
        if route == Route::Proxied && whole {
            spec.internet.get_or_insert_with(|| spec::Internet {
                hosts: Some(vec!["*".into()]),
                ..Default::default()
            });
        }
        spec
    }
}

/// An agent's `[files]` (the folders of its Home it writes and reads) as
/// a spec's, its files folder laid out as `folder`: Home written whole
/// (`.`) or read, each folder it names written (one of its own folder's
/// instructions, skills or conversations there), and those of its own
/// folder's it doesn't name read. Its teams' are its harness's to add.
pub fn files(files: &Folders, folder: &Folder) -> spec::Files {
    let text = |p: PathBuf| p.display().to_string();
    let mut write = Vec::new();
    let mut read = Vec::new();
    match files.whole() {
        true => write.push(text(folder.home.clone())),
        false => read.push(text(folder.home.clone())),
    }
    for name in files.write.iter().filter(|n| *n != WHOLE) {
        write.push(text(folder.seen(name)));
    }
    for collection in crate::folder::SEEN {
        if !files.write.iter().any(|n| n == collection.directory()) {
            read.push(text(folder.of(collection)));
        }
    }
    spec::Files {
        write: (!write.is_empty()).then_some(write),
        read: Some(read),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A namespace's name is Link's, handed to its harness under that
    /// name: a tool granted it would take its place.
    #[test]
    fn a_tool_cant_have_a_namespaces_name() {
        for name in link_harness_contract::harness::NAMESPACES {
            assert!(check_tool(name).is_err(), "{name}");
        }
        assert!(check_tool("tasks").is_ok());
        assert!(AgentSandbox::parse("schema = 1\n[tools]\nnames = [\"file\"]\n").is_err());
    }

    /// `text`, naming its schema where it names none.
    fn read(text: &str) -> AgentSandbox {
        match text.lines().any(|line| line.starts_with("schema")) {
            true => AgentSandbox::parse(text),
            false => AgentSandbox::parse(&format!("schema = 1\n{text}")),
        }
        .unwrap()
    }

    /// Link's file is a spec in the agent's terms, and reads back as
    /// written.
    #[test]
    fn links_file_is_a_spec() {
        let link = AgentSandbox::link();
        assert_eq!(link.folders(), Folders::default());
        assert_eq!(
            link.spec.internet.as_ref().unwrap().hosts.as_ref().unwrap(),
            &["*"]
        );
        let mac = link.spec.on(Os::MacOs).unwrap().commands.unwrap();
        assert_eq!(mac.developer_tools, Some(true));
        let again = read(&link.text("main").unwrap());
        assert_eq!(again, link);
    }

    /// Link's file grants the developer tools on a Mac alone, and a file
    /// that leaves them off grants them nowhere.
    #[test]
    fn the_developer_tools_are_granted_on_a_mac_alone() {
        assert_eq!(
            AgentSandbox::link().developer_tools(),
            Os::here() == Os::MacOs
        );
        let off = read("[commands]\nallow = [\"sh\"]\n[commands.macos]\ndeveloper_tools = false\n");
        assert!(!off.developer_tools());
    }

    #[test]
    fn a_file_is_read_strictly() {
        // Its own folder's read, never written, and it reads back the same.
        let locked =
            read("[files]\nwrite = [\"work\"]\nread = [\"instructions\", \"conversations\"]\n");
        assert_eq!(read(&locked.text("main").unwrap()), locked);
        for bad in [
            "unsandboxed = true\n",
            "[internet]\nhosts = [\"not a host\"]\n",
            "[internet]\nhosts = [\"*\"]\ntcp = [\"github.com\"]\n",
            "[internet]\nhosts = [\"*\"]\nper_minute = 0\n",
            "[localhost]\nports = [\"*\"]\n",
            "[localhost]\nmodels = [11434]\n",
            "[tools]\nnames = [\"memory-abcdefabcdef\"]\n",
            "[tools]\nnames = [\"team-abcdefabcdef-memory\"]\n",
            "[files]\nwrite = [\"chats\"]\n",
            "[files]\nwrite = [\"~\"]\n",
            "[files]\nread = [\"Documents/x\"]\n",
            "[files]\nwrite = [\"work\"]\nhide = [\"work/secrets\"]\n",
            // Dot files only at the top of a folder it writes.
            "[files]\nwrite_dot_files = [\".\"]\n",
            "[files]\nwrite = [\"work\"]\nwrite_dot_files = [\".\"]\n",
            "[commands]\nallow = [\"sh -c\"]\n",
            "[commands]\nallow = [\"/usr/bin/\"]\n",
            "[teams.research]\nwrite = [\"a/b\"]\n",
            // A folder it can't see, or one both written and read.
            "[files]\nwrite = [\"..\"]\n",
            "[files]\nwrite = [\"work\"]\nread = [\"work\"]\n",
            "[files]\nwrite = [\"instructions/notes\"]\n",
            "[files]\nrun = [\"work\"]\n",
            // Any beside others, which no sandbox takes.
            "[internet]\nhosts = [\"*\", \"api.openai.com\"]\n",
            "[browser]\nhosts = [\"a.com\", \"*\"]\n",
            "[commands]\nallow = [\"*\", \"sh\"]\n",
        ] {
            assert!(AgentSandbox::parse(bad).is_err(), "{bad:?}");
        }
        // An unknown key anywhere, the top's too, is refused, never
        // dropped beside the spec's sections and `[teams]`.
        for typo in [
            "intenet = 1\n",
            "[team.research]\nwrite = [\"a\"]\n",
            "[files]\nwirte = [\"work\"]\n",
        ] {
            assert!(AgentSandbox::parse(typo).is_err(), "{typo:?}");
        }
        // Every port, only on the network as it is.
        read(
            "[network]\nmode = \"host\"\n[internet]\nhosts = [\"*\"]\n[localhost]\nports = [\"*\"]\n",
        );
    }

    /// Laid out on a machine: Home read, its folders written, a routed
    /// agent behind its proxy whatever it says.
    #[test]
    fn its_spec_resolves_on_a_machine() {
        let folder = Folder::new(Path::new("/a/main"), None);
        let sandbox = read(
            "[files]\nwrite = [\"work\"]\nread = [\"~\"]\n[network]\nmode = \"host\"\n\
             [internet]\nhosts = [\"*\"]\n[localhost]\nports = [\"*\"]\n",
        );
        let harness = read("[browser]\nhosts = [\"*\"]\n[localhost]\nports = [5432]\n").spec;
        let spec = sandbox.resolve(&harness, &folder, Route::Direct);
        let files = spec.files.as_ref().unwrap();
        assert!(
            files
                .write
                .as_ref()
                .unwrap()
                .contains(&"/a/main/home/work".to_string())
        );
        assert!(
            files
                .read
                .as_ref()
                .unwrap()
                .contains(&"/a/main/home".to_string())
        );
        assert!(files.read.as_ref().unwrap().contains(&"~".to_string()));
        assert!(spec.browser.is_some());
        assert!(!spec.behind_proxy());
        assert_eq!(
            spec.localhost.as_ref().unwrap().ports.as_ref().unwrap(),
            &[Port::Word("*".into())]
        );
        // Its every host, joined with a harness's, stays every host.
        let hosts = read("[internet]\nhosts = [\"api.openai.com\"]\n").spec;
        let joined = sandbox.resolve(&hosts, &folder, Route::Direct);
        assert_eq!(joined.internet.unwrap().hosts.unwrap(), ["*"]);
        let routed = sandbox.resolve(&harness, &folder, Route::Proxied);
        assert!(routed.behind_proxy());
        assert_eq!(
            routed.localhost.unwrap().ports.unwrap(),
            [Port::Number(5432)]
        );
    }

    /// A coding agent granted its repository's dot files writes them at
    /// the top of its files folder, laid out there, so it commits; none
    /// without the grant.
    #[test]
    fn dot_files_granted_are_written_where_its_folder_is() {
        let folder = Folder::new(Path::new("/a/programmer"), Some(Path::new("/code/app")));
        let granted = read("[files]\nwrite = [\".\"]\nwrite_dot_files = [\".\"]\n");
        assert_eq!(read(&granted.text("programmer").unwrap()), granted);
        let spec = granted.resolve(&Spec::default(), &folder, Route::Direct);
        let files = spec.files.unwrap();
        assert_eq!(
            files.write_dot_files.unwrap(),
            [folder.home.display().to_string()]
        );
        let plain = read("[files]\nwrite = [\".\"]\n");
        let spec = plain.resolve(&Spec::default(), &folder, Route::Direct);
        assert!(spec.files.unwrap().write_dot_files.is_none());
    }

    /// Routed, an agent is behind its proxy, the internet through its
    /// route: never the network as it is, never `[lan]`, and never a host
    /// it isn't granted there (every host only for the whole network).
    #[test]
    fn a_routed_agent_is_always_behind_its_proxy() {
        let way = || {
            link_sandbox_client::Way::Proxy(link_sandbox_client::ProxyAt {
                upstream: "direct".into(),
                dir: "/n".into(),
                ..Default::default()
            })
        };
        let folder = Folder::new(Path::new("/a/main"), None);
        let host = "[network]\nmode = \"host\"\n[internet]\nhosts = [\"*\"]\n";
        let agents = [
            String::new(),
            "[internet]\nhosts = [\"a.com\"]\n".to_string(),
            "[lan]\nhosts = [\"*\"]\n".to_string(),
            host.to_string(),
            format!("{host}[localhost]\nports = [\"*\"]\n[browser]\nhosts = [\"*\"]\n"),
            "[localhost]\nports = [5432]\n[lan]\nhosts = [\"nas.local\"]\n".to_string(),
        ];
        let harnesses = [
            String::new(),
            format!("{host}[localhost]\nports = [\"*\"]\n"),
            "[lan]\nhosts = [\"192.168.1.0/24\"]\n[localhost]\nports = [8080]\n".to_string(),
        ];
        for agent in &agents {
            let sandbox = read(agent);
            for harness in &harnesses {
                let harness = match harness.is_empty() {
                    true => Spec::default(),
                    false => link_spec::parse::<Spec>(harness).unwrap(),
                };
                let mut runs = sandbox.resolve(&harness, &folder, Route::Proxied);
                if let Some(commands) = &mut runs.commands {
                    commands.allow = Some(vec!["/".into()]);
                }
                let c = link_sandbox_client::Confinement::of(
                    &runs,
                    link_sandbox_client::At {
                        name: "x",
                        dir: Path::new("/u/.config"),
                        home: Path::new("/u"),
                        path: None,
                        way: way(),
                    },
                )
                .unwrap_or_else(|err| panic!("{agent:?}: {err:#}"));
                // Behind it, no host the same spec on the network as it is
                // doesn't reach: every host only for the whole network. One
                // that reaches nothing at all is closed, no proxy needed.
                let whole = [&sandbox.spec, &harness]
                    .into_iter()
                    .any(|s| s.network.as_ref().and_then(|n| n.mode) == Some(NetworkMode::Host));
                let direct = sandbox.resolve(&harness, &folder, Route::Direct).internet;
                match &c.net {
                    link_sandbox_client::Net::Proxied { proxy, .. } => assert!(
                        proxy.no_internet == (direct.is_none() && !whole) && proxy.lan.is_empty(),
                        "{agent:?} with {harness:?}"
                    ),
                    link_sandbox_client::Net::Closed => assert!(
                        direct.is_none() && !whole,
                        "{agent:?} with {harness:?}: closed"
                    ),
                    net => panic!("{agent:?} with {harness:?}: {net:?}"),
                }
            }
        }
    }

    /// A deny holds on every route: Link's agent with every host taken
    /// away reaches none, on the network as it is or behind its proxy
    /// (`lnk agent route`), whatever its harness asks for but a host.
    #[test]
    fn a_host_taken_away_is_reached_on_no_route() {
        let folder = Folder::new(Path::new("/a/main"), None);
        let mut sandbox = AgentSandbox::link();
        let every: Spec = link_spec::parse("[internet]\nhosts = [\"*\"]\n").unwrap();
        sandbox.spec = sandbox.spec.without(&every);
        let harnesses = [
            Spec::default(),
            link_spec::parse::<Spec>("[localhost]\nports = [8080]\n").unwrap(),
        ];
        for harness in &harnesses {
            for route in [Route::Direct, Route::Proxied] {
                let spec = sandbox.resolve(harness, &folder, route);
                assert_eq!(spec.internet, None, "{route:?} with {harness:?}");
            }
        }
        // The loosening: granted one host, each route reaches it alone.
        let one: Spec = link_spec::parse("[internet]\nhosts = [\"a.com\"]\n").unwrap();
        sandbox.spec = sandbox.spec.join(&one);
        for route in [Route::Direct, Route::Proxied] {
            let hosts = sandbox.resolve(&Spec::default(), &folder, route).internet;
            assert_eq!(hosts.and_then(|i| i.hosts), Some(vec!["a.com".to_string()]));
        }
    }

    /// An agent's folders laid out on a machine: Home read, or written
    /// whole with `.`; each folder it names written; each of its own
    /// folder's it doesn't name read, never both.
    #[test]
    fn its_folders_are_laid_out_on_a_machine() {
        let folder = Folder {
            home: "/u/files".into(),
            agent: "/u/agent".into(),
        };
        let layout = |write: &[&str], read: &[&str]| {
            let laid = files(
                &Folders {
                    write: write.iter().map(|w| w.to_string()).collect(),
                    read: read.iter().map(|r| r.to_string()).collect(),
                    teams: BTreeMap::new(),
                },
                &folder,
            );
            (
                laid.write.unwrap_or_default(),
                laid.read.unwrap_or_default(),
            )
        };
        let own = ["/u/agent/instructions", "/u/agent/skills", "/u/agent/chats"];
        // Link's: Home read, `work` written, its own folders read.
        let (write, read) = layout(&["work"], &["chats"]);
        assert_eq!(write, ["/u/files/work"]);
        assert_eq!(read, [&["/u/files"][..], &own[..]].concat());
        // All of Home written, and `instructions` moved from read to
        // written, never both.
        let (write, read) = layout(&[".", "instructions"], &[]);
        assert_eq!(write, ["/u/files", "/u/agent/instructions"]);
        assert_eq!(read, ["/u/agent/skills", "/u/agent/chats"]);
        assert!(write.iter().all(|w| !read.contains(w)));
        // Nothing named: Home and its own folders read, nothing written.
        let (write, read) = layout(&[], &[]);
        assert!(write.is_empty());
        assert_eq!(read, [&["/u/files"][..], &own[..]].concat());
    }
}
