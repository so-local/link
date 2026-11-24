//! The contract between a module that confines a program (`lnk-agent`
//! for every harness) and the sandbox module (`lnk-sandbox`), which holds
//! the one sandbox every such program runs in.
//!
//! ```text
//! lnk sandbox wrap            {"confinement", "argv"} -> {"argv", "env"}: the command
//!                             that runs `argv` confined on this OS, and what to add
//!                             to its environment (a proxied one's proxy)
//! lnk sandbox check --ready exit 0 once the sandbox can run on this machine (asks,
//!                             in a terminal, to add what it needs; says what on stdout)
//! lnk sandbox check --policy  reads {"confinement"} on stdin, tries from inside it
//!                             what it shouldn't allow, and prints what got through;
//!                             exit status 1 when something did
//! lnk sandbox list --json     {"stopped", "sandboxes": [{"sandbox", "state", "paused"}]}:
//!                             each sandbox, and why its wire is closed ([`Wires`])
//! lnk sandbox run --with-fd N runs as `run` does, given more than a command line
//!                             should show on descriptor N ([`RunWith`])
//! ```
//!
//! A [`Confinement`], made from a [`Spec`] by one function
//! ([`Confinement::of`]), says what a program may touch beyond the
//! system's own programs and libraries: folders it writes and reads, the
//! programs it runs, and how it reaches the network ([`Net`]): not at
//! all, this machine's loopback, a [`Proxy`] (a network of its own whose
//! only way out is Link's filtering proxy), other sandboxes' proxies, or
//! the host's network as it is. Everything else is closed, and Link's
//! settings folder stays closed to reading and writing whatever the
//! confinement says: `wrap` refuses one with a folder inside it, or
//! writing a folder that holds it. The rules `wrap` writes (macOS) live
//! in [`rules_file`], in Link's settings, where nothing sandboxed can
//! edit them.

use link_secretive::Duplicate;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

pub mod confinement;
pub mod flags;
pub mod grants;
pub mod held;
pub mod namespace;
pub mod protected;
pub mod routes;
pub use link_sandbox_contract::{spec, system::LINUX_READS};
pub mod unwalled;
pub use confinement::{At, Confinement, Net, Programs, ProxyAt, Way};
pub use held::{HELD_LISTENER_ENV, HeldPort};
pub use spec::Spec;
pub use unwalled::Unwalled;

/// A proxy a sandbox reaches that another sandbox's runs
/// ([`Net::Reaching`]): its sockets' folder, and the port inside its
/// program reaches it at.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reached {
    pub dir: PathBuf,
    pub port: u16,
}

/// Link's filtering proxy, a sandbox's only way out: it refuses this
/// machine and its networks (a cloud's metadata service too), and dials
/// the rest from here or through an upstream.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proxy {
    /// Where its connections leave from: `direct`,
    /// `socks5://127.0.0.1:<port>` or `socks5://<socket>` ([`crate::net::Upstream`]).
    pub upstream: String,
    /// A folder (only the user's) for the sockets between the sandbox and
    /// the proxy, outside it. One sandbox at a time uses it.
    pub dir: PathBuf,
    /// The ports the program serves, served on this machine's loopback
    /// too, so health checks and its own pages reach it.
    #[serde(default)]
    pub serve: BTreeSet<u16>,
    /// The only hosts it may reach ([`crate::net::check_host`]: `example.com`, or
    /// `*.example.com` for the names under it); empty, any on the
    /// internet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    /// Endpoints beyond HTTPS and HTTP it may reach raw (`host:port`,
    /// [`crate::net::check_endpoint`]): `github.com:22`, `db.example.com:5432`,
    /// `*.abc.mongodb.net:27017` for the names under one. Its
    /// proxy's `CONNECT` reaches only port 443, and a plain request only
    /// port 80, but for these; `hosts`, if any, still applies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tcp: Vec<String>,
    /// The port its program reaches its proxy at, known before it starts,
    /// so the program can be set up with its proxy's URLs (its routes'):
    /// on macOS this machine's
    /// loopback, none a free one; on Linux the sandbox's own network,
    /// none [`PROXY_PORT`] (a port of its own where several agents'
    /// proxies are reached from one sandbox, [`Net::Reaching`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen: Option<u16>,
    /// The most calls its proxy takes within a minute, of every verb;
    /// more get a 429 until the minute has room. None: no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_minute: Option<u32>,
    /// Its routes ([`crate::route`]): each way out beside the network (a
    /// model, a secret, a tool, a page to open), mounted at a prefix of
    /// `/_link/` and served by a bridge outside.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[duplicate]
    pub routes: Vec<crate::route::Route>,
    /// Who its routes' calls are from, proved to each route's bridge: an
    /// agent's binding, of the keyring its bridges check. None: one of
    /// the wrap's own, bound in this machine's keyring for the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[duplicate]
    pub caller: Option<crate::bridge::Binding>,
    /// The secret the URLs of its routes that need one carry
    /// ([`crate::route::Route::needs_url_secret`]): the proxy takes those
    /// only with it, since on macOS any program of the machine can reach
    /// the proxy's port. The caller keeps it to hand the program those
    /// URLs itself ([`crate::route::url_secret`]); None: one of the wrap's
    /// own, in the variables it hands out (each route's `env`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub secret: Option<link_secretive::Secret>,
    /// Machines of your network it may reach through the proxy
    /// ([`crate::net::check_lan`]): a name, an address, a range (`192.168.1.0/24`),
    /// or `*` for the private ranges. Never this machine, a link-local
    /// address or a cloud's metadata service; dialed from here, whatever
    /// the upstream.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lan: Vec<String>,
    /// No internet: only `lan`, its routes and ports.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_internet: bool,
    /// It runs alone, holding no program (Linux): the sandbox it serves
    /// is started apart, reaching it at `listen` ([`Net::Reaching`]), so
    /// wrapping gives only the proxy's command, and the program's
    /// environment as it would reach it. Never with `tcp` or ports to
    /// reach, which are the sandbox's own network's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hold: bool,
}

impl Proxy {
    /// Its route named `name`, if it has one.
    pub fn route(&self, name: &str) -> Option<&crate::route::Route> {
        self.routes.iter().find(|route| route.name == name)
    }

    /// The tools it may call (its tools route's).
    pub fn tools(&self) -> Vec<String> {
        self.route("mcp")
            .and_then(|r| crate::route::given::<routes::Tools>(&r.given).ok())
            .map(|t| t.tools)
            .unwrap_or_default()
    }

    /// This machine's model runtimes it reaches (its local model route's).
    pub fn models(&self) -> BTreeSet<u16> {
        self.route("local-model")
            .map(|r| r.ports.clone())
            .unwrap_or_default()
    }

    /// The hosts a page it opens may be on (`["*"]`: any), when it opens
    /// pages (its open route's).
    pub fn pages(&self) -> Option<Vec<String>> {
        self.route("open")
            .and_then(|r| crate::route::given::<routes::Open>(&r.given).ok())
            .map(|o| o.hosts)
    }
}

/// What binds a blank proxy to one agent (`lnk sandbox proxy --blank
/// --control <socket>`, Linux): one line of JSON on its control socket.
/// What its command line takes, by the same names, but what it started
/// with (its folder, its program, `--hold`, `--dns`), with its routes and
/// who calls them whole, the credentials they add among them, in place of
/// the files they'd be read from: none is ever written to the disk. A
/// blank proxy answers every call 503 until it's bound, and is bound
/// once: the binding ends with the process.
#[derive(Duplicate, Default, PartialEq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct ProxyBind {
    /// Its sandbox's name on its wire (`<agent>-<harness>`).
    #[serde(default)]
    pub name: String,
    /// Link's settings folder its wire is logged in; none, this user's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire: Option<PathBuf>,
    /// [`Proxy::upstream`].
    pub upstream: String,
    /// [`Proxy::hosts`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host: Vec<String>,
    /// [`Proxy::tcp`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tcp: Vec<String>,
    /// [`Proxy::serve`]: served on this machine's loopback, so none for
    /// a sandbox reached only through its folder.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub serve: BTreeSet<u16>,
    /// This machine's ports it reaches (its confinement's `ports`).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub reach: BTreeSet<u16>,
    /// What a desktop's session needs to tell you something (`NAME=VALUE`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub desktop: Vec<String>,
    /// [`Proxy::lan`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lan: Vec<String>,
    /// [`Proxy::no_internet`].
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_internet: bool,
    /// [`Proxy::per_minute`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_minute: Option<u32>,
    /// [`Proxy::routes`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[duplicate]
    pub routes: Vec<crate::route::Route>,
    /// [`Proxy::caller`]: needed with any route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[duplicate]
    pub caller: Option<crate::bridge::Binding>,
    /// [`Proxy::secret`], itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub url_secret: Option<link_secretive::Secret>,
    /// The run's own secret, new at each launch, which a `CONNECT` or a
    /// plain `http://` request carries as its `Proxy-Authorization`: a
    /// proxy on this machine's loopback takes none without it, so a
    /// process that outlived an earlier run never uses a later run's
    /// proxy at the same port. None: a proxy only its sandbox reaches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub proxy_secret: Option<link_secretive::Secret>,
}

/// What a blank proxy's control socket answers a [`ProxyBind`]: `{}`
/// once bound, or why not (`{"error"}`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProxyBound {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Binds the blank proxy whose control socket is `control` with `bind`:
/// an error when it refuses (it's bound already, or `bind` doesn't
/// hold), saying why.
pub fn bind_proxy(control: &Path, bind: &ProxyBind) -> anyhow::Result<()> {
    use std::io::{BufRead, Read};
    let mut protected = bind.duplicate();
    if let Some(caller) = &protected.caller {
        crate::secrets::keeper::protect_routes(&mut protected.routes, caller)?;
    }
    let bind = &protected;
    let mut socket = std::os::unix::net::UnixStream::connect(control)
        .with_context(|| format!("reaching the proxy at {}", control.display()))?;
    socket.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut line = serde_json::to_string(bind)?;
    line.push('\n');
    socket.write_all(line.as_bytes())?;
    let mut answer = String::new();
    std::io::BufReader::new(socket)
        .take(64 * 1024)
        .read_line(&mut answer)
        .context("reading the proxy's answer")?;
    let bound: ProxyBound = serde_json::from_str(&answer).context("reading the proxy's answer")?;
    match bound.error {
        None => Ok(()),
        Some(why) => bail!("the proxy wasn't bound: {}", link_format::clean(&why)),
    }
}

/// The proxy's port inside a sandbox's own network (Linux).
pub const PROXY_PORT: u16 = 40781;

/// Folders no sandbox writes (a spec's, an adapter's `Launch`): what
/// runs from them runs outside the sandbox, or they're the system's. Homebrew's are `/opt/homebrew`,
/// `/usr/local` and `/home/linuxbrew`.
pub const SYSTEM_FOLDERS: &[&str] = &[
    "/bin",
    "/sbin",
    "/usr",
    "/etc",
    "/lib",
    "/lib32",
    "/lib64",
    "/boot",
    "/dev",
    "/proc",
    "/sys",
    "/opt",
    "/nix",
    "/snap",
    "/var",
    "/home/linuxbrew",
    "/System",
    "/Library",
    "/Applications",
    "/private/etc",
    "/private/var",
];

/// Where programs keep their unix sockets (a session's bus, Docker's,
/// tmux's), which a sandbox that reads or writes them could connect to:
/// on Linux a read-only folder still connects, and what answers runs
/// outside. Neither a spec nor an adapter's `Launch` names them.
pub fn socket_folders() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = [
        "/tmp",
        "/private/tmp",
        "/run",
        "/var/run",
        "/private/var/run",
        "/var/folders",
        "/private/var/folders",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    folders.extend(
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute()),
    );
    folders.push(std::env::temp_dir());
    folders
}

/// Whether `path` has no control character (a newline, say): nothing
/// needs one, and a sandbox's rules are text.
pub fn plain_path(path: &std::path::Path) -> bool {
    !path.to_string_lossy().chars().any(char::is_control)
}

/// What `lnk sandbox run --with-fd <N>` reads, as JSON, on descriptor N
/// (a pipe the caller wrote and closed), never on its command line: the
/// variables the program gets, the secrets its proxy adds, the ports it
/// serves and folders it writes beside its spec's. A secret is given as
/// on a harness's proxy: the program reaches its service at the variable
/// `url` names ([`secret_url`]), with the placeholder the variable
/// `placeholder` names ([`Secret::placeholder`]) where the secret would
/// be. Generated from the client's declarations (`types.toml`), with
/// the routes it names and the secret it seals.
pub use crate::declared::{RunRoutes, RunSecret, RunWith};

impl Duplicate for RunWith {
    fn duplicate(&self) -> Self {
        Self {
            env: self.env.clone(),
            named: self.named.as_ref().map(Duplicate::duplicate),
            secrets: self.secrets.iter().map(Duplicate::duplicate).collect(),
            serve: self.serve.clone(),
            write: self.write.clone(),
        }
    }
}

impl Duplicate for RunRoutes {
    fn duplicate(&self) -> Self {
        Self {
            caller: self.caller.duplicate(),
            routes: self.routes.iter().map(Duplicate::duplicate).collect(),
        }
    }
}

impl Duplicate for RunSecret {
    fn duplicate(&self) -> Self {
        Self {
            git: self.git,
            placeholder: self.placeholder.clone(),
            secret: self.secret.duplicate(),
            url: self.url.clone(),
        }
    }
}

impl RunRoutes {
    /// Persisted tool definitions may name secrets, never retain values.
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.routes.is_empty(), "named run routes are empty");
        anyhow::ensure!(
            self.caller.key.expose().len() >= 32,
            "run route caller has no binding key"
        );
        anyhow::ensure!(
            !self.caller.identity.agent.is_empty() && !self.caller.identity.owner.is_empty(),
            "run route caller has no identity"
        );
        link_sandbox_binding::check_agent(&self.caller.identity.agent)?;
        link_sandbox_binding::check_owner(&self.caller.identity.owner)?;
        let mut prefixes = BTreeSet::new();
        let mut variables = BTreeSet::new();
        for route in &self.routes {
            route.check()?;
            route.names_only()?;
            for (name, value) in &route.env {
                crate::check_given_variable(name)?;
                anyhow::ensure!(
                    !value.contains('\0'),
                    "named route variable {name} has a NUL"
                );
                anyhow::ensure!(
                    variables.insert(name),
                    "two named routes set the same variable"
                );
            }
            anyhow::ensure!(
                prefixes.insert(&route.prefix),
                "two run routes use the same prefix"
            );
        }
        Ok(())
    }
}

pub use link_sandbox_contract::check_given_variable;

/// What `lnk sandbox wrap` reads: what to confine `argv` to, and
/// `argv`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct WrapRequest<C = Confinement, U = Unwalled> {
    #[serde(flatten)]
    pub sandbox: Wrap<C, U>,
    pub argv: Vec<String>,
    /// Its proxy's routes printed back ([`Handing`]) for a caller that
    /// starts what it's given at once, to hand on a pipe: none written.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hand: bool,
}

/// What `lnk sandbox wrap` prints for a request that hands: what to run,
/// and what its caller hands it as it starts it ([`Handed::hand_to`]).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Handing {
    #[serde(flatten)]
    pub wrapped: Wrapped,
    #[serde(default, skip_serializing_if = "Handed::is_empty")]
    pub handed: Handed,
}

/// Secrets by the variable a program reads them by, its proxy's routes
/// among them: handed on a pipe as it starts, never on the disk, in its
/// environment or a command line. Never printed.
#[derive(Default)]
pub struct Handed(pub Vec<(String, link_secretive::Secret)>);

impl Handed {
    /// Whether there's nothing to hand.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Hands each to `command`, after its environment is set
    /// ([`link_secretive::process::hand_to`]).
    pub fn hand_to(&self, command: &mut std::process::Command) {
        link_secretive::process::hand_to(command, self.0.iter().map(|(n, s)| (n.as_str(), s)));
    }
}

impl std::fmt::Debug for Handed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|(name, _)| name))
            .finish()
    }
}

impl Serialize for Handed {
    fn serialize<S: serde::Serializer>(&self, to: S) -> Result<S::Ok, S::Error> {
        to.collect_map(self.0.iter().map(|(name, secret)| (name, secret.expose())))
    }
}

impl<'de> Deserialize<'de> for Handed {
    fn deserialize<D: serde::Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        let given = BTreeMap::<String, String>::deserialize(from)?;
        Ok(Handed(
            given
                .into_iter()
                .map(|(name, value)| (name, link_secretive::Secret::new(value)))
                .collect(),
        ))
    }
}

/// What a [`WrapRequest`] confines its program to, by its key (`lnk
/// sandbox check --policy` reads it alone): a confinement, or no wall and
/// a proxy beside it; each, or a reference to one where it's sent. A
/// policy, as callers sent before confinements, is refused.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wrap<C = Confinement, U = Unwalled> {
    Confinement(C),
    /// No wall: the program as it is, its proxy beside it, which it may
    /// ignore ([`Unwalled`]). Never checked: there's nothing to check.
    Unwalled(U),
}

/// What `lnk sandbox wrap` prints.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Wrapped {
    pub argv: Vec<String>,
    /// To add to the program's environment: with a proxy, where it is
    /// (`HTTPS_PROXY`, ...).
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Where the rules for sandbox `name` are kept (macOS: a Seatbelt profile),
/// in Link's settings folder.
pub fn rules_file(name: &str) -> PathBuf {
    link_files::layout::Hidden::Sandboxes
        .path()
        .join(format!("{name}.sb"))
}

/// What sandbox `name`'s proxy is handed through Link's settings, beside
/// its [`rules_file`]: its routes, which may hold credentials, and the
/// secret its URLs carry. Needed only while it runs.
pub fn secret_files(name: &str) -> [PathBuf; 2] {
    let rules = rules_file(name);
    [
        rules.with_extension("routes.json"),
        rules.with_extension("url-secret"),
    ]
}

/// Every file `wrap` keeps for sandbox `name` in Link's settings: its
/// [`rules_file`] and its [`secret_files`].
pub fn files(name: &str) -> [PathBuf; 3] {
    let [routes, url_secret] = secret_files(name);
    [rules_file(name), routes, url_secret]
}

/// Removes sandbox `name`'s [`secret_files`], once its proxy has ended.
pub fn forget_secrets(name: &str) {
    for file in secret_files(name) {
        let _ = std::fs::remove_file(file);
    }
}

/// Removes every file of sandbox `name`'s ([`files`]), once it has ended.
pub fn forget(name: &str) {
    for file in files(name) {
        let _ = std::fs::remove_file(file);
    }
}

/// Installs the sandbox module (`lnk module add sandbox`) if it's missing,
/// as for a module installed before it was one of its own.
pub fn ensure() -> anyhow::Result<()> {
    crate::modules::add("sandbox", "Running an agent").map(drop)
}

/// An error unless `name` is a sandbox's: letters, digits and `-`, not
/// first, as a rules file, a wire's log and a pause are named after it.
pub fn check_name(name: &str) -> anyhow::Result<()> {
    let plain = !name.is_empty()
        && !name.starts_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    anyhow::ensure!(plain, "{name:?} isn't a sandbox's name");
    Ok(())
}

/// `argv`, wrapped to run inside `confinement` (`lnk sandbox wrap`), and
/// what to add to its environment.
pub fn wrap(confinement: &Confinement, argv: &[String]) -> anyhow::Result<Wrapped> {
    confinement.check_name()?;
    Ok(wrap_as(Wrap::Confinement(confinement), argv, false)?.wrapped)
}

/// [`wrap`], for a caller that starts what it returns at once: its
/// proxy's routes, which may hold credentials, come back to hand it as it
/// starts ([`Handed::hand_to`]), never written to a file.
pub fn wrap_handing(confinement: &Confinement, argv: &[String]) -> anyhow::Result<Handing> {
    confinement.check_name()?;
    wrap_as(Wrap::Confinement(confinement), argv, true)
}

/// `argv` run with no wall, its proxy beside it (`lnk sandbox wrap`): the
/// proxy's command running it, and what to add to its environment.
pub fn wrap_unwalled(unwalled: &Unwalled, argv: &[String]) -> anyhow::Result<Wrapped> {
    check_name(&unwalled.name)?;
    Ok(wrap_as(Wrap::Unwalled(unwalled), argv, false)?.wrapped)
}

/// [`wrap_unwalled`], handing its proxy's routes as [`wrap_handing`] does.
pub fn wrap_unwalled_handing(unwalled: &Unwalled, argv: &[String]) -> anyhow::Result<Handing> {
    check_name(&unwalled.name)?;
    wrap_as(Wrap::Unwalled(unwalled), argv, true)
}

/// `lnk sandbox wrap` of `sandbox` and `argv`.
fn wrap_as(
    sandbox: Wrap<&Confinement, &Unwalled>,
    argv: &[String],
    hand: bool,
) -> anyhow::Result<Handing> {
    ensure()?;
    let request = WrapRequest {
        sandbox,
        argv: argv.to_vec(),
        hand,
    };
    let output = with_input(
        crate::lnk()
            .args(["sandbox", "wrap"])
            .stdout(Stdio::piped()),
        &request,
    )?;
    if !output.status.success() {
        bail!("lnk sandbox wrap failed (above)");
    }
    let handing: Handing =
        serde_json::from_slice(&output.stdout).context("reading what lnk sandbox wrap printed")?;
    if handing.wrapped.argv.is_empty() {
        bail!("lnk sandbox wrap printed no command");
    }
    Ok(handing)
}

/// Makes sure the sandbox can run here (`lnk sandbox check --ready`), on this
/// terminal.
pub fn ready() -> anyhow::Result<()> {
    ensure()?;
    let status = crate::lnk()
        .args(["sandbox", "check", "--ready"])
        .status()
        .context("running lnk sandbox check --ready")?;
    if !status.success() {
        bail!("the sandbox can't run on this machine (above)");
    }
    Ok(())
}

/// Tries from inside `confinement` what it shouldn't allow (`lnk sandbox
/// check --policy`), its report on this terminal. Whether nothing got
/// through.
pub fn check(confinement: &Confinement) -> anyhow::Result<bool> {
    check_reading(confinement, &[], &[])
}

/// [`check`], also trying to read each of `reads`, which it must, and
/// each of `unread`, which it must not (a team's folder, and another
/// team's).
pub fn check_reading(
    confinement: &Confinement,
    reads: &[std::path::PathBuf],
    unread: &[std::path::PathBuf],
) -> anyhow::Result<bool> {
    confinement.check_name()?;
    ensure()?;
    let mut check = crate::lnk();
    check.args(["sandbox", "check", "--policy"]);
    for folder in reads {
        check.arg("--reads").arg(folder);
    }
    for folder in unread {
        check.arg("--refuses-read").arg(folder);
    }
    let output = with_input(&mut check, &Wrap::<_, Unwalled>::Confinement(confinement))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!("lnk sandbox check failed (above)"),
    }
}

/// Runs `command` with `input` as JSON on its stdin, its errors on this
/// terminal.
fn with_input(
    command: &mut Command,
    input: &impl Serialize,
) -> anyhow::Result<std::process::Output> {
    let mut child = command
        .stdin(Stdio::piped())
        .spawn()
        .context("running lnk")?;
    child
        .stdin
        .take()
        .context("no input to lnk")?
        .write_all(serde_json::to_string(input)?.as_bytes())?;
    Ok(child.wait_with_output()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_variables_cannot_inject_startup_code() {
        for name in [
            "BASH_ENV",
            "ENV",
            "NODE_OPTIONS",
            "NODE_PATH",
            "PYTHONPATH",
            "PYTHONHOME",
            "PYTHONSTARTUP",
            "PERL5OPT",
            "PERL5LIB",
            "RUBYOPT",
            "RUBYLIB",
            "JAVA_TOOL_OPTIONS",
            "_JAVA_OPTIONS",
            "JDK_JAVA_OPTIONS",
            "GCONV_PATH",
            "LOCPATH",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "HTTPS_PROXY",
            "LNK_SANDBOX_DIR",
            "PATH",
            "HOME",
        ] {
            assert!(check_given_variable(name).is_err(), "{name}");
            assert!(
                check_given_variable(&name.to_ascii_lowercase()).is_err(),
                "{name}"
            );
        }
        for name in ["API_URL", "BUILD_MODE", "MY_SETTING", "GITHUB_TOKEN"] {
            check_given_variable(name).unwrap();
        }
    }

    #[test]
    fn a_bind_holds_its_routes_and_caller_inline_and_shows_none() {
        let bind: ProxyBind = serde_json::from_str(
            r#"{"name":"slot-1","upstream":"direct","url_secret":"0123456789abcdef",
                "serve":[18800],"caller":{"identity":{"agent":"a1","owner":"o1"},
                "key":"the-agent-key"},"routes":[{"name":"model","prefix":"/_link/model",
                "bridge":{"socket":"/run/b/models.sock"},
                "given":"{\"key\":\"sk-ant-the-key\"}"}]}"#,
        )
        .unwrap();
        assert_eq!(bind.caller.as_ref().unwrap().key.expose(), "the-agent-key");
        assert!(bind.routes[0].given.expose().contains("sk-ant-the-key"));
        let shown = format!("{bind:?}");
        for never in ["the-key", "the-agent-key", "0123456789abcdef"] {
            assert!(!shown.contains(never), "{shown}");
        }
        // Only what the command line takes: a file of a key, a program,
        // or a field of a route the proxy doesn't name, isn't a bind's.
        for wrong in [
            r#"{"upstream":"direct","model_key":{}}"#,
            r#"{"upstream":"direct","argv":["sh"]}"#,
            r#"{"upstream":"direct","routes":[{"name":"x","prefix":"/_link/x",
                "bridge":{"socket":"/s"},"given":"{}","verbs":[]}]}"#,
        ] {
            assert!(serde_json::from_str::<ProxyBind>(wrong).is_err(), "{wrong}");
        }
        let bound: ProxyBound = serde_json::from_str("{}").unwrap();
        assert_eq!(bound.error, None);
        assert_eq!(serde_json::to_string(&bound).unwrap(), "{}");
    }

    #[test]
    fn paths_and_names_are_plain() {
        assert!(plain_path(Path::new("/Users/me/My Files")));
        for bad in ["/x\")\n(allow default)", "/x\ry", "/x\u{7f}"] {
            assert!(!plain_path(Path::new(bad)), "{bad:?}");
        }
        let named = |name: &str| Confinement {
            name: name.into(),
            write: Vec::new(),
            write_dot_files: Vec::new(),
            read: Vec::new(),
            hide: Vec::new(),
            read_home: false,
            developer_tools: false,
            builds: Vec::new(),
            tmp_prefix: None,
            terminal: false,
            commands: Programs::Any,
            net: Net::Closed,
        };
        assert!(named("otter-setup").check_name().is_ok());
        for bad in ["", "../x", "a/b", "-x", "a b"] {
            assert!(named(bad).check_name().is_err(), "{bad:?}");
        }
        let wrapped: Wrapped = serde_json::from_str(r#"{"argv":["x"]}"#).unwrap();
        assert!(wrapped.env.is_empty());
    }

    #[test]
    fn a_wire_is_closed_when_stopped_or_paused() {
        let mut wires: crate::Wires = serde_json::from_str(
            r#"{"stopped":false,"paused":[{"sandbox":"main-otter","why":"by a test"}]}"#,
        )
        .unwrap();
        assert_eq!(
            wires.closed("main-otter").as_deref(),
            Some("paused, by a test (lnk sandbox start main-otter)")
        );
        assert_eq!(wires.closed("main-heron"), None);
        wires.stopped = true;
        assert!(wires.closed("main-heron").unwrap().contains("stopped"));
    }
}
