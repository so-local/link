//! A sandbox as its backend confines a program: a [`Spec`] resolved for
//! this OS, every path absolute, every key checked, and its network one
//! of the [`Net`]s, so a combination no sandbox can run can't be written.
//!
//! [`Confinement::of`] is the one function that makes one: from a spec,
//! and [`At`], what isn't a grant (its name, where the spec's paths are
//! from, and how its network is carried). What a caller adds beside the
//! spec (an agent's own folder, Link's hidden folders, a terminal) it
//! sets on the result.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};

use super::spec::{NetworkMode, Os, Port, Spec};
use super::{PROXY_PORT, Proxy, Reached, check_name};
use link_secretive::Duplicate;

/// What a sandboxed program may touch, on this OS.
#[derive(Duplicate, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confinement {
    /// Its name (letters, digits, dashes), naming its rules file.
    pub name: String,
    /// Read and write.
    pub write: Vec<PathBuf>,
    /// Of `write`, the folders whose dot files that run outside the
    /// sandbox (`.git`, `.envrc`, `.vscode`, `.idea`) it may write too.
    #[serde(default)]
    pub write_dot_files: Vec<PathBuf>,
    /// Read only.
    pub read: Vec<PathBuf>,
    /// The whole home folder, read only (`[files] read = ["~"]`).
    #[serde(default)]
    pub read_home: bool,
    /// Closed whatever else is allowed, but for what `write` and `read`
    /// name inside them. Link's settings are closed always.
    #[serde(default)]
    pub hide: Vec<PathBuf>,
    /// Files and folders in `/tmp` whose names start with this, or a dot
    /// and this, read and written (macOS; Linux's `/tmp` is its own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmp_prefix: Option<String>,
    /// The terminal it runs on is its own: it keeps it as its controlling
    /// terminal. Never for a terminal something reads after it.
    #[serde(default)]
    pub terminal: bool,
    /// The programs it may run: always said, so a request that leaves
    /// it out is refused rather than read as any.
    pub commands: Programs,
    /// Xcode's and the Command Line Tools' programs, read only and run (macOS).
    #[serde(default)]
    pub developer_tools: bool,
    /// Folders it writes whose programs it may run, each under a folder of
    /// `write`, or one: its toolchains' builds, and a folder its caller
    /// installs into, for an installer that runs what it downloads there.
    /// In `commands` too, and made as the
    /// sandbox starts, since a folder that isn't there can't be granted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub builds: Vec<PathBuf>,
    #[duplicate]
    pub net: Net,
}

pub use link_sandbox_contract::Programs;

/// How a sandbox reaches the network: each a mode of its own, holding
/// only what that mode takes.
#[expect(
    clippy::large_enum_variant,
    reason = "one a sandbox, made once: its size doesn't matter"
)]
#[derive(Duplicate, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Net {
    /// No network at all.
    Closed,
    /// Only `ports` of this machine, on its loopback: no proxy, no
    /// internet. On Linux a network of its own joined to those ports by a
    /// relay whose sockets are in `dir` (only the user's), one sandbox at a
    /// time; on macOS its rules alone.
    Loopback { ports: BTreeSet<u16>, dir: PathBuf },
    /// Its only way out is its proxy, which reaches `ports` of this
    /// machine for it too, and its routes (a page opened in your browser
    /// among them, asking you).
    Proxied {
        #[duplicate]
        proxy: Proxy,
        ports: BTreeSet<u16>,
    },
    /// Its only ways out are other sandboxes' proxies, each run apart and
    /// reached at its own port inside (Linux), and `ports` of this machine.
    Reaching {
        reached: Vec<Reached>,
        ports: BTreeSet<u16>,
    },
    /// This machine's network as it is, with no proxy: the internet and
    /// the local network unfiltered, and of this machine's loopback every
    /// port (`localhost`) or only `ports` (macOS; Linux's is shared).
    Host {
        ports: BTreeSet<u16>,
        localhost: bool,
    },
}

/// What a [`Confinement`] is made with beside its spec.
pub struct At<'a> {
    /// Its name ([`Confinement::name`]).
    pub name: &'a str,
    /// Where the spec's relative paths are from: its file's folder.
    pub dir: &'a Path,
    pub home: &'a Path,
    /// Where `[commands]` finds a program it names: a `PATH`. None: a
    /// name is kept as written, for a confinement that's only checked,
    /// never run (`lnk sandbox spec show`).
    pub path: Option<&'a OsStr>,
    /// How its network is carried, when its spec opens one.
    pub way: Way,
}

/// How a sandbox's network is carried, when its spec opens one.
#[expect(
    clippy::large_enum_variant,
    reason = "one a sandbox, made once: its size doesn't matter"
)]
pub enum Way {
    /// Through a proxy of its own ([`Net::Proxied`]); a spec with no
    /// network gets none ([`Net::Closed`]).
    Proxy(ProxyAt),
    /// Its spec's ports on the loopback alone, relayed through this
    /// folder ([`Net::Loopback`]).
    Loopback(PathBuf),
    /// Through these proxies, run apart ([`Net::Reaching`]).
    Reaching(Vec<Reached>),
}

/// What a sandbox's proxy is given beside what its spec opens: where it
/// runs, the routes its caller adds (a hosted model's, its secrets') and
/// who calls them ([`Proxy`]'s fields of the same names).
#[derive(Duplicate, Debug, Default, PartialEq)]
pub struct ProxyAt {
    pub upstream: String,
    pub dir: PathBuf,
    pub serve: BTreeSet<u16>,
    pub listen: Option<u16>,
    #[duplicate]
    pub routes: Vec<crate::route::Route>,
    #[duplicate]
    pub caller: Option<crate::bridge::Binding>,
    #[duplicate]
    pub secret: Option<link_secretive::Secret>,
    pub hold: bool,
    /// Whom its questions are asked for, and how long each waits; None:
    /// you alone, on this machine, as long as the sandbox's settings say.
    pub asking: Option<crate::sandbox::routes::Asking>,
}

/// What a spec's network sections open, checked.
#[derive(Default)]
pub(super) struct Opened {
    /// `[internet] hosts`: None, closed; empty, any public host.
    pub(super) internet: Option<Vec<String>>,
    pub(super) tcp: Vec<String>,
    pub(super) per_minute: Option<u32>,
    pub(super) lan: Vec<String>,
    ports: BTreeSet<u16>,
    /// `[localhost] ports = ["*"]`.
    every_port: bool,
    models: BTreeSet<u16>,
    tools: Vec<String>,
    /// `[browser] hosts`: None, closed; empty, any.
    pub(super) browser: Option<Vec<String>>,
}

impl Opened {
    fn any(&self) -> bool {
        self.internet.is_some()
            || !self.lan.is_empty()
            || !self.ports.is_empty()
            || !self.models.is_empty()
            || !self.tools.is_empty()
            || self.browser.is_some()
    }

    /// An error if it opens more than this machine's ports, which `way`
    /// (a sandbox with no proxy of its own) can't carry.
    fn ports_only(&self, way: &str) -> anyhow::Result<()> {
        ensure!(
            self.internet.is_none()
                && self.lan.is_empty()
                && self.models.is_empty()
                && self.tools.is_empty()
                && self.browser.is_none(),
            "{way} reaches this machine's ports alone: no [internet], [lan], [tools], \
             [browser] or [localhost] models"
        );
        Ok(())
    }
}

impl Confinement {
    /// The confinement `spec` makes on this OS, made [`At`]: each
    /// section's required key and every `"*"` checked, its paths made
    /// absolute (from `at.dir` and `at.home`), and `[commands]` found on
    /// `at.path`. What needs this machine's sandbox to judge (a write that
    /// leads out of it, a program it could replace) the sandbox checks as
    /// it wraps.
    pub fn of(spec: &Spec, at: At) -> anyhow::Result<Confinement> {
        let spec = spec.on(Os::here())?;
        ensure!(
            spec.teams.is_empty(),
            "[teams] is an agent's grant: only its harness knows where a team's shared folder \
             is, so a sandbox run of its own takes none"
        );
        let path = |p: &String| link_spec::path(Path::new(p), at.dir, at.home);
        let paths = |ps: &Option<Vec<String>>, key: &str| -> anyhow::Result<Vec<PathBuf>> {
            let ps = ps.as_deref().unwrap_or_default();
            ensure!(
                !ps.iter().any(|p| p == "*"),
                "[files] {key} can't be \"*\": name the folders"
            );
            ps.iter().map(path).collect()
        };
        let mut confinement = Confinement {
            name: at.name.to_string(),
            write: Vec::new(),
            write_dot_files: Vec::new(),
            read: Vec::new(),
            read_home: false,
            hide: Vec::new(),
            tmp_prefix: None,
            terminal: false,
            commands: Programs::Only(Vec::new()),
            developer_tools: false,
            builds: Vec::new(),
            net: Net::Closed,
        };
        if let Some(files) = &spec.files {
            ensure!(
                files.write.is_some() || files.read.is_some(),
                "[files] needs write or read"
            );
            confinement.write = paths(&files.write, "write")?;
            confinement.write_dot_files = paths(&files.write_dot_files, "write_dot_files")?;
            for folder in &confinement.write_dot_files {
                ensure!(
                    confinement.write.contains(folder),
                    "[files] write_dot_files {}: not a folder of its write, whose dot files it \
                     would open",
                    folder.display()
                );
            }
            let read = paths(&files.read, "read")?;
            confinement.read_home = read.iter().any(|p| p == at.home);
            confinement.read = read.into_iter().filter(|p| p != at.home).collect();
            confinement.hide = paths(&files.hide, "hide")?;
        }
        let host = spec.network.as_ref().and_then(|n| n.mode) == Some(NetworkMode::Host);
        let opened = opened(&spec, host)?;
        confinement.commands = match &spec.commands {
            None => Programs::Only(Vec::new()),
            Some(commands) => {
                let allow = commands.allow.as_deref().unwrap_or_default();
                let toolchains = commands.toolchains.as_deref().unwrap_or_default();
                ensure!(
                    !allow.is_empty() || !toolchains.is_empty(),
                    "[commands] needs allow or toolchains"
                );
                // `["*"]` is any program, until a sandbox made for
                // toolchains and their builds replaces it.
                let any = matches!(allow, [only] if only == "*");
                let mut programs: Vec<PathBuf> = allow
                    .iter()
                    .filter(|_| !any)
                    .map(|entry| program(entry, at.dir, at.home, at.path))
                    .collect::<anyhow::Result<_>>()?;
                for name in toolchains {
                    let here = link_sandbox_contract::toolchains::named(*name).here(
                        at.home,
                        at.path,
                        &confinement.write,
                    );
                    programs.extend(here.programs);
                    confinement.builds.extend(here.builds);
                    for dir in here.read {
                        if !confinement.read_home && !confinement.read.contains(&dir) {
                            confinement.read.push(dir);
                        }
                    }
                }
                programs.extend(confinement.builds.iter().cloned());
                let mut seen = BTreeSet::new();
                programs.retain(|program| seen.insert(program.clone()));
                match any {
                    true => Programs::Any,
                    false => Programs::Only(programs),
                }
            }
        };
        confinement.developer_tools = spec
            .commands
            .as_ref()
            .and_then(|c| c.developer_tools)
            .unwrap_or(false);
        confinement.net = match (host, at.way) {
            (true, Way::Proxy(_)) => {
                let filtered = opened.internet.as_ref().is_some_and(|h| !h.is_empty())
                    || !opened.tcp.is_empty()
                    || opened.per_minute.is_some()
                    || !opened.lan.is_empty()
                    || !opened.models.is_empty()
                    || !opened.tools.is_empty()
                    || opened.browser.is_some();
                ensure!(
                    !filtered,
                    "with [network] mode = \"host\" nothing filters its network: [internet] \
                     hosts can only be [\"*\"], and [lan], [tools], [browser], tcp, per_minute \
                     and [localhost] models have nothing to filter them (name a model's port in \
                     ports)"
                );
                Net::Host {
                    ports: opened.ports,
                    localhost: opened.every_port,
                }
            }
            (true, _) => bail!(
                "[network] mode = \"host\" is a sandbox's own network, which one on the loopback \
                 or reaching other proxies doesn't have"
            ),
            (false, Way::Loopback(dir)) => {
                opened.ports_only("a sandbox on the loopback alone")?;
                Net::Loopback {
                    ports: opened.ports,
                    dir,
                }
            }
            (false, Way::Reaching(reached)) => {
                opened.ports_only("a sandbox reaching other proxies")?;
                Net::Reaching {
                    reached,
                    ports: opened.ports,
                }
            }
            (false, Way::Proxy(_)) if !opened.any() => Net::Closed,
            (false, Way::Proxy(at)) => Net::Proxied {
                ports: opened.ports.clone(),
                proxy: proxy(opened, at)?,
            },
        };
        Ok(confinement)
    }

    /// An error unless the name is one a rules file can be named after.
    pub fn check_name(&self) -> anyhow::Result<()> {
        check_name(&self.name)
    }

    /// The machines of your network its proxy reaches (`[lan] hosts`).
    pub fn lan(&self) -> &[String] {
        match &self.net {
            Net::Proxied { proxy, .. } => &proxy.lan,
            _ => &[],
        }
    }
}

/// The sections of `spec` (on one OS) that open a network, checked; with
/// `host`, `[localhost] ports = ["*"]` is taken.
pub(super) fn opened(spec: &Spec, host: bool) -> anyhow::Result<Opened> {
    let mut opened = Opened::default();
    if let Some(internet) = &spec.internet {
        let hosts = required(&internet.hosts, "[internet] needs hosts")?;
        opened.internet = Some(star_or(hosts, crate::net::check_host)?);
        for endpoint in internet.tcp.iter().flatten() {
            crate::net::check_endpoint(endpoint)?;
        }
        opened.tcp = internet.tcp.clone().unwrap_or_default();
        opened.per_minute = internet.per_minute;
        ensure!(
            opened.per_minute != Some(0),
            "[internet] per_minute is at least 1"
        );
    }
    if let Some(lan) = &spec.lan {
        let hosts = required(&lan.hosts, "[lan] needs hosts")?;
        for host in hosts {
            crate::net::check_lan(host)?;
        }
        opened.lan = hosts.clone();
    }
    if let Some(localhost) = &spec.localhost {
        // A model's port alone is a section of its own: inference only.
        let ports = localhost.ports.as_deref().unwrap_or_default();
        ensure!(
            !ports.is_empty() || localhost.models.as_ref().is_some_and(|m| !m.is_empty()),
            "[localhost] needs ports or models"
        );
        for port in ports {
            match port {
                Port::Number(0) => bail!("[localhost] port 0 isn't a port"),
                Port::Number(n) => {
                    opened.ports.insert(*n);
                }
                Port::Word(star) if star == "*" && host => opened.every_port = true,
                // Every port would be every agent's proxy too, each
                // another agent's way out.
                Port::Word(star) if star == "*" => bail!(
                    "[localhost] ports = [\"*\"] is taken only with [network] mode = \"host\": \
                     behind a proxy every port would reach other sandboxes' proxies; name the \
                     ports"
                ),
                Port::Word(other) => bail!("[localhost] {other:?} isn't a port"),
            }
        }
        ensure!(
            !opened.every_port || ports.len() == 1,
            "\"*\" is everything already: leave out the rest"
        );
        for model in localhost.models.iter().flatten() {
            ensure!(*model != 0, "[localhost] model port 0 isn't a port");
            opened.models.insert(*model);
        }
        // An agent's proxy, another sandbox's way out.
        ensure!(
            !opened.ports.contains(&PROXY_PORT) && !opened.models.contains(&PROXY_PORT),
            "[localhost] port {PROXY_PORT} is Link's proxy's, another sandbox's way out"
        );
    }
    if let Some(tools) = &spec.tools {
        let names = required(&tools.names, "[tools] needs names")?;
        for name in names {
            ensure!(
                name != "*",
                "[tools] names can't be \"*\": each tool runs with your logins, so name them"
            );
            ensure!(
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
                "{name:?} isn't a tool's name"
            );
        }
        opened.tools = names.clone();
    }
    if let Some(browser) = &spec.browser {
        let hosts = required(&browser.hosts, "[browser] needs hosts")?;
        opened.browser = Some(star_or(hosts, crate::net::check_host)?);
    }
    let taken: BTreeSet<u16> = opened.ports.union(&opened.models).copied().collect();
    wildcard_ports_free(&opened.tcp, &taken)?;
    Ok(opened)
}

/// The proxy a spec's network sections make, given what it's run with:
/// the one place a [`Proxy`] is made from a spec. Its routes are the
/// caller's, then those its spec opens, each served by its module's
/// bridge here: `[tools]`'s and `[browser]`'s the sandbox's own, `[localhost]
/// models`' the models module's.
pub(super) fn proxy(opened: Opened, at: ProxyAt) -> anyhow::Result<Proxy> {
    use crate::sandbox::routes;
    let mut routes = at.routes;
    let desktop = crate::desktop::desktop();
    if !opened.tools.is_empty() {
        routes.push(routes::tools(
            routes::bridge()?,
            routes::Tools {
                tools: opened.tools,
                desktop: desktop.clone(),
                asking: at.asking.clone(),
            },
        )?);
    }
    if !opened.models.is_empty() {
        routes.push(crate::route::local_model::local(
            crate::route::local_model::bridge()?,
            opened.models,
        )?);
    }
    if let Some(hosts) = opened.browser {
        // Any host is `*`: an empty list is the internet's hosts.
        let hosts = match hosts.is_empty() {
            true => vec!["*".into()],
            false => hosts,
        };
        routes.push(routes::open(
            routes::bridge()?,
            routes::Open {
                hosts,
                desktop,
                asking: at.asking.clone(),
            },
        )?);
    }
    crate::route::check_routes(&routes)?;
    Ok(Proxy {
        hold: at.hold,
        upstream: at.upstream,
        dir: at.dir,
        serve: at.serve,
        no_internet: opened.internet.is_none(),
        hosts: opened.internet.unwrap_or_default(),
        tcp: opened.tcp,
        listen: at.listen,
        per_minute: opened.per_minute,
        routes,
        caller: at.caller,
        secret: at.secret,
        lan: opened.lan,
    })
}

/// A `[commands] allow` entry as a path: a name found on `path` and
/// resolved to its real executable (kept as written without a PATH),
/// or an explicit path from the spec's folder `dir` or `~/`. Discovery
/// follows installed links once; the wall still checks explicit grants.
fn program(entry: &str, dir: &Path, home: &Path, path: Option<&OsStr>) -> anyhow::Result<PathBuf> {
    match (entry.contains('/'), path) {
        (true, _) => link_spec::path(Path::new(entry), dir, home),
        (false, None) => Ok(PathBuf::from(entry)),
        (false, Some(path)) => {
            let program = link_files::find_program(entry, path)
                .ok_or_else(|| anyhow::anyhow!("[commands] {entry}: not found on PATH"))?;
            program
                .canonicalize()
                .with_context(|| format!("[commands] {entry}: resolving {}", program.display()))
        }
    }
}

fn required<'a, T>(value: &'a Option<Vec<T>>, why: &str) -> anyhow::Result<&'a Vec<T>> {
    match value {
        Some(v) if !v.is_empty() => Ok(v),
        _ => bail!("{why}"),
    }
}

/// `["*"]` as an empty list (everything of its kind), else each entry
/// checked; `"*"` beside others is refused.
fn star_or(
    list: &[String],
    check: impl Fn(&str) -> anyhow::Result<()>,
) -> anyhow::Result<Vec<String>> {
    if list.iter().any(|e| e == "*") {
        ensure!(
            list.len() == 1,
            "\"*\" is everything already: leave out the rest"
        );
        return Ok(Vec::new());
    }
    for entry in list {
        check(entry)?;
    }
    Ok(list.to_vec())
}

/// An error if a `*.` endpoint of `tcp` is on a port of `taken` (this
/// machine's ports the sandbox reaches) or its proxy's: inside, such an
/// endpoint listens on that port on every loopback address, where those
/// already listen.
pub fn wildcard_ports_free(tcp: &[String], taken: &BTreeSet<u16>) -> anyhow::Result<()> {
    for endpoint in tcp {
        let (host, port) = crate::net::check_endpoint(endpoint)?;
        if !host.starts_with("*.") {
            continue;
        }
        ensure!(
            !taken.contains(&port) && port != PROXY_PORT,
            "[internet] tcp {endpoint}: port {port} is one of this machine's it reaches, or its \
             proxy's, and a *. endpoint takes its port on every loopback address"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(text: &str, way: Way) -> anyhow::Result<Confinement> {
        let spec: Spec = link_spec::parse_spec(&format!("schema = 1\n{text}"))?;
        let bin = std::env::var_os("PATH").unwrap_or_default();
        Confinement::of(
            &spec,
            At {
                name: "x",
                dir: Path::new("/specs"),
                home: Path::new("/home/me"),
                path: Some(&bin),
                way,
            },
        )
    }

    fn proxied() -> Way {
        Way::Proxy(ProxyAt {
            upstream: "direct".into(),
            dir: "/s".into(),
            ..Default::default()
        })
    }

    #[test]
    fn a_spec_with_no_sections_opens_nothing() {
        let c = of("name = \"x\"\nversion = 1\n", proxied()).unwrap();
        assert_eq!(c.net, Net::Closed);
        assert_eq!(c.commands, Programs::Only(vec![]));
        assert!(c.write.is_empty() && c.read.is_empty() && !c.read_home);
    }

    #[test]
    fn each_section_needs_its_key_and_stars_only_where_bounded() {
        for (text, why) in [
            ("[internet]\ntcp = [\"a.com:22\"]", "needs hosts"),
            ("[files]\nhide = [\"x\"]", "needs write or read"),
            ("[commands]\nallow = []", "needs allow"),
            ("[commands]\ndeveloper_tools = true", "needs allow"),
            ("[files]\nwrite = [\"*\"]", "can't be \"*\""),
            ("[tools]\nnames = [\"*\"]", "can't be \"*\""),
            ("[localhost]\nports = [\"*\"]", "only with [network] mode"),
            ("[localhost]\nmodels = []", "needs ports or models"),
            ("[localhost]\n", "[localhost]"),
            ("[localhost]\nports = [40781]", "Link's proxy"),
            (
                "[internet]\nhosts = [\"*\", \"a.com\"]",
                "leave out the rest",
            ),
            (
                "[internet]\nhosts = [\"*\"]\ntcp = [\"*.a.com:40781\"]",
                "every loopback address",
            ),
            ("[internet]\nhosts = [\"*\"]\ntcp = [\"a.com\"]", "endpoint"),
            ("[lan]\nhosts = [\"10.0.0.0/99\"]", "more bits"),
            ("[internet]\nhost = [\"*\"]", "unknown field"),
            ("[teams.research]\nread = [\".\"]", "an agent's grant"),
            (
                "[internet]\nhosts = [\"*\"]\ntcp = [\"*.a.com:8080\"]\n[localhost]\nports = [8080]",
                "every loopback address",
            ),
        ] {
            let err = format!("{:#}", of(text, proxied()).unwrap_err());
            assert!(err.contains(why), "{text}: {err}");
        }
    }

    #[test]
    fn its_network_is_one_mode_and_holds_only_that_modes_keys() {
        let c = of(
            "[internet]\nhosts = [\"*\"]\n[localhost]\nports = [8080]\nmodels = [11434]\n\
             [browser]\nhosts = [\"*\"]",
            proxied(),
        )
        .unwrap();
        let Net::Proxied { proxy, ports } = &c.net else {
            panic!("{:?}", c.net)
        };
        assert!(proxy.pages().is_some() && proxy.hosts.is_empty() && !proxy.no_internet);
        let open: crate::sandbox::routes::Open =
            crate::route::given(&proxy.route("open").unwrap().given).unwrap();
        assert_eq!(open.hosts, ["*"]);
        assert_eq!(*ports, BTreeSet::from([8080]));
        let models = |proxy: &Proxy| proxy.route("local-model").map(|r| r.ports.clone());
        assert_eq!(models(proxy), Some(BTreeSet::from([11434])));
        // A model alone: its proxy, for inference only.
        let c = of("[localhost]\nmodels = [11434]", proxied()).unwrap();
        assert!(
            matches!(&c.net, Net::Proxied { proxy, ports, .. } if ports.is_empty() && models(proxy) == Some(BTreeSet::from([11434])))
        );
        // Its ports alone: still its proxy, with no internet.
        let c = of("[localhost]\nports = [8080]", proxied()).unwrap();
        assert!(matches!(&c.net, Net::Proxied { proxy, .. } if proxy.no_internet));
        // Host: every port, only there; nothing to filter.
        let host = "[network]\nmode = \"host\"\n";
        let c = of(&format!("{host}[localhost]\nports = [\"*\"]"), proxied()).unwrap();
        assert_eq!(
            c.net,
            Net::Host {
                ports: BTreeSet::new(),
                localhost: true
            }
        );
        let c = of(&format!("{host}[internet]\nhosts = [\"*\"]"), proxied()).unwrap();
        assert!(matches!(
            c.net,
            Net::Host {
                localhost: false,
                ..
            }
        ));
        for refused in [
            "[internet]\nhosts = [\"a.com\"]",
            "[lan]\nhosts = [\"*\"]",
            "[browser]\nhosts = [\"*\"]",
            "[localhost]\nports = [1]\nmodels = [11434]",
        ] {
            assert!(
                of(&format!("{host}{refused}"), proxied()).is_err(),
                "{refused}"
            );
        }
        assert!(of(host, Way::Loopback("/l".into())).is_err());
        // The loopback, and other proxies: this machine's ports alone.
        let c = of("[localhost]\nports = [8080]", Way::Loopback("/l".into())).unwrap();
        assert_eq!(
            c.net,
            Net::Loopback {
                ports: BTreeSet::from([8080]),
                dir: "/l".into()
            }
        );
        assert!(of("[internet]\nhosts = [\"*\"]", Way::Loopback("/l".into())).is_err());
        assert!(of("[tools]\nnames = [\"gh\"]", Way::Reaching(Vec::new())).is_err());
        let c = of("", Way::Reaching(Vec::new())).unwrap();
        assert!(matches!(c.net, Net::Reaching { .. }));
    }

    /// A toolchain stands for its programs this machine has, and its
    /// builds' folders in each folder written that is a project of its
    /// kind, which it runs too.
    #[test]
    fn a_toolchain_is_its_programs_and_its_builds() {
        let root = link_test_kit::TempDir::new("toolchain-confinement");
        let (app, notes) = (root.join("app"), root.join("notes"));
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(app.join("Cargo.toml"), "").unwrap();
        let spec: Spec = link_spec::parse(&format!(
            "[files]\nwrite = [\"{}\", \"{}\"]\n[commands]\nallow = [\"/usr/bin/\"]\ntoolchains = [\"rust\"]",
            app.display(),
            notes.display()
        ))
        .unwrap();
        let at = || At {
            name: "x",
            dir: Path::new("/specs"),
            home: Path::new("/home/me"),
            path: None,
            way: proxied(),
        };
        let c = Confinement::of(&spec, at()).unwrap();
        assert_eq!(c.builds, [app.join("target/")]);
        let runs = c.commands.listed().unwrap();
        for program in [
            PathBuf::from("/usr/bin/"),
            "cargo".into(),
            "rustc".into(),
            app.join("target/"),
        ] {
            assert!(runs.contains(&program), "{program:?}: {runs:?}");
        }
        assert!(
            Confinement::of(
                &link_spec::parse("[commands]\ndeveloper_tools = true").unwrap(),
                at()
            )
            .is_err(),
            "a section names programs or toolchains"
        );
    }

    #[test]
    fn paths_are_made_absolute_and_home_read_is_its_own() {
        let c = of(
            "[files]\nwrite = [\"./app\", \"~/code\"]\nread = [\"~\"]\nwrite_dot_files = [\"~/code\"]\n\
             [commands]\nallow = [\"./bin/run\", \"/usr/bin/\"]\n[commands.macos]\nallow = [\"./bin/run\"]\n\
             developer_tools = true",
            proxied(),
        )
        .unwrap();
        assert_eq!(
            c.write,
            [PathBuf::from("/specs/app"), "/home/me/code".into()]
        );
        assert!(c.read_home && c.read.is_empty());
        let runs = |c: &Confinement| c.commands.clone().listed().unwrap();
        if cfg!(target_os = "macos") {
            assert!(c.developer_tools);
            assert_eq!(runs(&c), [PathBuf::from("/specs/bin/run")]);
        } else {
            assert!(!c.developer_tools);
            assert_eq!(runs(&c).len(), 2);
        }
        // `["*"]` is any program, until a sandbox for toolchains replaces it.
        let any = of("[commands]\nallow = [\"*\"]", proxied()).unwrap();
        assert_eq!(any.commands, Programs::Any);
        assert!(of("[commands]\ntoolchains = [\"cobol\"]", proxied()).is_err());
        assert!(of("[commands]\nallow = [\"no-such-program-here\"]", proxied()).is_err());
        // Only checked, never run: a name kept as written.
        let spec: Spec =
            link_spec::parse("[commands]\nallow = [\"no-such-program-here\"]").unwrap();
        let at = At {
            name: "x",
            dir: Path::new("/specs"),
            home: Path::new("/home/me"),
            path: None,
            way: proxied(),
        };
        let kept = Confinement::of(&spec, at).unwrap();
        assert_eq!(
            kept.commands,
            Programs::Only(vec![PathBuf::from("no-such-program-here")])
        );
        assert!(
            of(
                "[files]\nwrite = [\"./a\"]\nwrite_dot_files = [\"./b\"]",
                proxied()
            )
            .is_err()
        );
    }

    #[test]
    fn a_confinement_is_sent_by_its_key() {
        let c = of(
            "[files]\nread = [\"~\"]\n[localhost]\nports = [8080]\nmodels = [11434]\n\
             [browser]\nhosts = [\"a.com\"]",
            proxied(),
        )
        .unwrap();
        // As JSON, the mode names its keys.
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["net"]["proxied"]["proxy"]["routes"][1]["name"], "open");
        // `lnk sandbox wrap` reads a confinement, by its key; a policy, as
        // callers sent before confinements, is refused.
        use super::super::{Wrap, WrapRequest};
        let sent = serde_json::json!({"confinement": json, "argv": ["sh"]});
        let new: WrapRequest = serde_json::from_value(sent.clone()).unwrap();
        assert!(matches!(new.sandbox, Wrap::Confinement(n) if n == c));
        assert!(
            serde_json::from_str::<WrapRequest>(r#"{"policy": {"name": "a"}, "argv": ["sh"]}"#)
                .is_err()
        );
        // Its programs always said: left out, refused, never any.
        let mut left_out = sent;
        left_out["confinement"]
            .as_object_mut()
            .unwrap()
            .remove("commands");
        assert!(serde_json::from_value::<WrapRequest>(left_out).is_err());
        // Its proxy's keys too: an unknown one is refused.
        let mut typo = json;
        typo["net"]["proxied"]["proxy"]["hosst"] = serde_json::json!(["a.com"]);
        assert!(serde_json::from_value::<Confinement>(typo).is_err());
    }
}
