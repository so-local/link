//! A route: a way out of a sandbox beside the network, which its proxy
//! mounts at a prefix of `/_link/` and a bridge outside serves (a hosted
//! model's, a secret's service, a tool, a page to open). The proxy names
//! none: it matches a call's target to a route, checks the URL secret if
//! the route needs it, proves who calls to the route's bridge
//! ([`link_sandbox_bridge`]'s stream call) and hands the call over; the bridge
//! says its verdict ([`Verdict`]), which the proxy logs, and answers.
//!
//! What every side shares is here: a route as its caller binds it
//! ([`Route`], the bridge it goes to, [`Bridge`]), a call as the proxy
//! hands it ([`Call`]), the placeholders a program holds in place of a
//! credential a route adds ([`Placeholders`]), the URL secret, and the
//! kit both sides read and dial with: the one HTTP parser ([`http`]),
//! idle-bounded copies ([`io`]), dialing out ([`dial`], [`resolve`]),
//! and TLS to a route's one host on connections kept open ([`tls`],
//! [`pool`]).
//! Each route's own types are its module's: a model's in
//! [`crate::models::route`], a secret's in [`crate::secrets::route`], the
//! sandbox's own in [`crate::sandbox::routes`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::ensure;
use serde::{Deserialize, Serialize};

use link_secretive::Duplicate;

pub mod bridge;
pub mod dial;
pub mod http;
pub mod io;
mod placeholders;
pub mod pool;
pub mod resolve;
pub mod tls;

pub use placeholders::{PLACEHOLDER, Placeholders, check_placeholder};

/// A way out a proxy mounts, as its caller binds it: where, which
/// bridge serves it, and what that bridge is given for each call.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    /// Its name, as the log keeps it: `model`, `secret`, `mcp`.
    pub name: String,
    /// The target it serves: a prefix ending in `/` (`/_link/mcp/`), or
    /// a name (`/_link/model`), served as itself and with anything after
    /// a `/` or `?`.
    pub prefix: String,
    /// Whether its URL carries the proxy's URL secret
    /// (`/_link/url/<secret>/...`), refused without it: a route whose URL
    /// a program is handed, since on macOS any program of the machine can
    /// reach the proxy's port.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub needs_url_secret: bool,
    /// This machine's ports it serves too, a connection to one of them a
    /// call ([`To::Port`]): on Linux each is relayed into the sandbox's
    /// network (a model runtime's, for inference only).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub ports: BTreeSet<u16>,
    /// The bridge that serves it.
    pub bridge: Bridge,
    /// What its bridge is given with each call, its own JSON (a model's
    /// key, a secret and its rule, the tools it may call): held as a
    /// secret, since it may hold one, and never read by the proxy.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub given: link_secretive::Secret,
    /// What the program's environment gets for it (`LNK_TOOL_GH`), each
    /// value with `{proxy}` (the proxy's root, `http://127.0.0.1:<port>`),
    /// `{secret}` (its URL secret) and `{exe}` (the sandbox's program,
    /// where a variable names a program) filled in.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Whether `given` holds a secret's value, handed with a proxy started
    /// or bound at once: never in a file, where a route names its
    /// secrets, each read by its bridge from the store at each call.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub handed: bool,
}

/// A route's bridge: where it listens, and what starts it when nothing
/// does (one per module per machine, started by the first call to need
/// it and gone once idle).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bridge {
    pub socket: PathBuf,
    /// Its program and arguments; none, nothing starts it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub start: Vec<String>,
}

impl Bridge {
    /// The bridge `module` serves (`lnk-<module> [<group>] bridge`, with no
    /// group for a module whose program never reads its namespace) for this
    /// machine's user: its socket in the runtime folder, and
    /// started with them and this machine's keyring, both handed, never
    /// found from the environment it starts in. A module that isn't
    /// installed has nothing to start it, which its calls say.
    pub fn of(module: &str, group: Option<&str>) -> anyhow::Result<Self> {
        let settings = link_files::try_installation_dir()?;
        let runtime = link_files::layout::Root::Runtime.try_path()?;
        let keys = link_sandbox_binding::Keyring::here();
        Ok(match crate::find_module(module) {
            Some(program) => Self::at(&runtime, &settings, &program, module, group, keys.dir()),
            None => Self {
                socket: Self::socket(&runtime, module),
                start: Vec::new(),
            },
        })
    }

    /// Where `module`'s bridge listens for the user of Link's `settings`.
    pub fn socket(runtime: &Path, module: &str) -> PathBuf {
        link_files::layout::Runtime::Bridges
            .at(runtime)
            .join(format!("{module}.sock"))
    }

    /// The bridge of `module`, whose `program` serves it as `group`, its
    /// socket in `runtime`, its calls proved against the
    /// keyring in `keys`.
    pub fn at(
        runtime: &Path,
        settings: &Path,
        program: &Path,
        module: &str,
        group: Option<&str>,
        keys: &Path,
    ) -> Self {
        let socket = Self::socket(runtime, module);
        let text = |p: &Path| p.display().to_string();
        Self {
            start: [text(program)]
                .into_iter()
                .chain(group.map(str::to_string))
                .chain([
                    "bridge".into(),
                    "--socket".into(),
                    text(&socket),
                    "--keys".into(),
                    text(keys),
                    "--settings".into(),
                    text(settings),
                    "--runtime".into(),
                    text(runtime),
                ])
                .collect(),
            socket,
        }
    }
}

impl Route {
    /// An error if it holds a secret's value ([`Route::handed`]), which a
    /// routes file never does.
    pub fn names_only(&self) -> anyhow::Result<()> {
        ensure!(
            !self.handed,
            "the {} route holds a secret's value: a route kept in a file names its secrets",
            self.name
        );
        Ok(())
    }

    /// Whether it serves `target`.
    pub fn serves(&self, target: &str) -> bool {
        let Some(after) = target.strip_prefix(self.prefix.as_str()) else {
            return false;
        };
        self.prefix.ends_with('/') || after.is_empty() || after.starts_with(['/', '?'])
    }

    /// An error unless a proxy can mount it: a plain name, a prefix of
    /// Link's own (`/_link/`, never behind the URL secret's), an absolute
    /// socket, and its variables' names plain (capitals, digits and `_`).
    pub fn check(&self) -> anyhow::Result<()> {
        let plain = |text: &str| {
            !text.is_empty()
                && text.len() <= 40
                && text
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        };
        ensure!(
            plain(&self.name),
            "{:?} isn't a route's name",
            link_format::clean(&self.name)
        );
        let rest = self.prefix.strip_prefix("/_link/").unwrap_or_default();
        ensure!(
            !rest.is_empty()
                && !self.prefix.starts_with(SECRET_PREFIX)
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-/".contains(&b)),
            "{:?} isn't a route's prefix: one of /_link/'s",
            link_format::clean(&self.prefix)
        );
        ensure!(
            self.bridge.socket.is_absolute(),
            "route {}'s bridge isn't at an absolute path",
            self.name
        );
        for name in self.env.keys() {
            let plain = name.starts_with(|c: char| c.is_ascii_uppercase())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
            ensure!(
                plain,
                "{:?} isn't a variable's name",
                link_format::clean(name)
            );
        }
        Ok(())
    }

    /// What the program's environment gets for it, its proxy's root
    /// `proxy`, its URL secret `secret`, and `exe` the sandbox's program:
    /// a variable naming `{exe}` left out when it's none.
    pub fn env_for(
        &self,
        proxy: &str,
        secret: &str,
        exe: Option<&str>,
    ) -> BTreeMap<String, String> {
        self.env
            .iter()
            .filter(|(_, value)| exe.is_some() || !value.contains("{exe}"))
            .map(|(name, value)| {
                let value = value
                    .replace("{proxy}", proxy)
                    .replace("{secret}", secret)
                    .replace("{exe}", exe.unwrap_or_default());
                (name.clone(), value)
            })
            .collect()
    }
}

/// What a route's bridge is given ([`Route::given`], [`Call::given`]),
/// read as its own type `T`.
pub fn given<T: serde::de::DeserializeOwned>(given: &link_secretive::Secret) -> anyhow::Result<T> {
    serde_json::from_str(given.expose())
        .map_err(|err| anyhow::anyhow!("reading the route's given: {err}"))
}

/// An error unless `routes` can be mounted together: each one that can,
/// none two of a name or a prefix, nor two serving a port.
pub fn check_routes(routes: &[Route]) -> anyhow::Result<()> {
    let mut names = BTreeSet::new();
    let mut prefixes = BTreeSet::new();
    let mut ports = BTreeSet::new();
    for route in routes {
        route.check()?;
        ensure!(names.insert(&route.name), "two routes named {}", route.name);
        ensure!(
            prefixes.insert(&route.prefix),
            "two routes at {}",
            route.prefix
        );
        for port in &route.ports {
            ensure!(ports.insert(*port), "two routes serve port {port}");
        }
    }
    Ok(())
}

/// One call on a route, as the proxy hands it to the route's bridge,
/// after proving who calls: the action of a stream call
/// ([`link_sandbox_bridge::stream`]). What the call carries (a request's body)
/// follows it, as it comes.
#[derive(Duplicate, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    /// The route's name.
    pub route: String,
    /// The sandbox's name on its wire (`<agent>-<harness>`): who asks, as
    /// a question shows it.
    pub sandbox: String,
    /// Where the bridge's own connections for the call leave from: the
    /// sandbox's ([`crate::net::Upstream`]), so they go out as its own.
    pub upstream: String,
    /// Whether the sandbox reaches only the hosts it lists: then what
    /// would fetch a page for it past them (a provider's own tools) is
    /// refused.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub listed: bool,
    /// The route's [`Route::given`].
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub given: link_secretive::Secret,
    pub to: To,
    /// The socket into the sandbox's own network (Linux), for a call that
    /// serves one of this machine's ports into it a while (a login's
    /// callback); none where the sandbox shares this machine's loopback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbound: Option<Inbound>,
}

/// What a call is to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum To {
    /// An HTTP request: its head, through its blank line, its target
    /// past the URL secret (`/_link/url/<secret>/mcp/gh` is `/_link/mcp/gh`).
    Request { head: String },
    /// A connection to one of [`Route::ports`] of this machine, its first
    /// bytes what follows.
    Port { port: u16 },
}

/// A bridge's verdict on a call, before it answers: what the log keeps.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    /// Where the call goes, as the log keeps it: never a path, which can
    /// carry data; a host and port, a provider, a tool and its call.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,
    /// How it went.
    #[serde(default)]
    pub outcome: Outcome,
}

/// How a call went, as its bridge says before it answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// It goes through: what the bridge answers is its answer.
    #[default]
    Allowed,
    /// The bridge refused it (its rules, or you), and answers the
    /// program why itself.
    Refused,
    /// Allowed, but what it asked for couldn't be reached: the bridge
    /// answers the program why itself.
    Failed,
}

impl Verdict {
    /// A call to `to` that goes through.
    pub fn allowed(to: impl Into<String>) -> Self {
        Self {
            to: to.into(),
            outcome: Outcome::Allowed,
        }
    }

    /// A call to `to` refused.
    pub fn refused(to: impl Into<String>) -> Self {
        Self {
            to: to.into(),
            outcome: Outcome::Refused,
        }
    }

    /// A call to `to` that couldn't reach it.
    pub fn failed(to: impl Into<String>) -> Self {
        Self {
            to: to.into(),
            outcome: Outcome::Failed,
        }
    }
}

/// Where the routes that are given as URLs are: behind the proxy's URL
/// secret, `/_link/url/<secret>/`, then the route's prefix past `/_link/`.
pub const SECRET_PREFIX: &str = "/_link/url/";

/// A new URL secret: 32 hexadecimal characters.
pub fn url_secret() -> std::io::Result<String> {
    link_files::random_token()
}

/// Whether `secret` could be one of [`url_secret`]'s: 16 to 64 letters
/// and digits, nothing a URL would read otherwise.
pub fn is_url_secret(secret: &str) -> bool {
    (16..=64).contains(&secret.len()) && secret.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Where a route at `prefix` is for a program whose proxy's root is
/// `{proxy}`, behind its URL secret when it `needs_url_secret`: a value
/// for [`Route::env`].
pub fn url_of(prefix: &str, needs_url_secret: bool) -> String {
    let rest = prefix.strip_prefix("/_link/").unwrap_or(prefix);
    match needs_url_secret {
        true => format!("{{proxy}}{SECRET_PREFIX}{{secret}}/{rest}"),
        false => format!("{{proxy}}/_link/{rest}"),
    }
}

/// A sandbox's `in.sock`, reached from outside as its agent's socket
/// alone ([`connect_inside`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inbound {
    pub path: std::path::PathBuf,
    /// The agent's user, whose socket it is.
    pub agent: u32,
}

impl Inbound {
    pub async fn connect(&self) -> std::io::Result<tokio::net::UnixStream> {
        connect_inside(&self.path, self.agent).await
    }
}

/// Outside: connects to `path`, a socket the sandbox made in a folder it
/// writes (`in.sock`), as that socket itself: never through a link the
/// sandbox put in its place (to the session bus, Docker's socket), and
/// only to a socket of `owner`'s, the sandbox's user.
/// Linux opens the path without following it and connects through the
/// open file, so nothing swapped in between is reached.
pub async fn connect_inside(path: &Path, owner: u32) -> std::io::Result<tokio::net::UnixStream> {
    let socket =
        link_files::nofollow::socket(path, |uid| uid == owner).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => std::io::Error::new(
                e.kind(),
                format!("{} isn't the sandbox's socket", path.display()),
            ),
            _ => e,
        })?;
    tokio::net::UnixStream::connect(&socket.at).await
}

/// Where a route's bridge says its verdict on a call, before anything it
/// answers: the call's stream ([`link_sandbox_bridge::stream::Stream`]), or a
/// connection that takes none ([`Unsaid`]).
pub trait Verdicts {
    fn say(
        &mut self,
        verdict: &Verdict,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send;
}

impl Verdicts for link_sandbox_bridge::stream::Stream {
    async fn say(&mut self, verdict: &Verdict) -> std::io::Result<()> {
        self.verdict(verdict).await
    }
}

/// A connection a route's answer is written to with no verdict before
/// it: what a route's code is tested on.
pub struct Unsaid<S>(pub S);

impl<S: Send> Verdicts for Unsaid<S> {
    async fn say(&mut self, _: &Verdict) -> std::io::Result<()> {
        Ok(())
    }
}

impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Unsaid<S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for Unsaid<S> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

/// A connection that may tell a peer that left from one that only shut
/// its writing (a half-close) after its request, and still waits for the
/// answer: a route's bridge watches it while a call waits (a tool's).
pub trait Hangup {
    /// The socket to ask whether its peer hung up entirely
    /// ([`io::hung_up`]); `None` when the two look alike, and its end of
    /// what it sends counts as leaving.
    fn hangup_socket(&self) -> Option<std::os::fd::RawFd>;
}

/// The proxy closes a call's stream whole once its client hung up, and
/// Linux tells that from shutting its writing alone.
impl Hangup for link_sandbox_bridge::stream::Stream {
    fn hangup_socket(&self) -> Option<std::os::fd::RawFd> {
        cfg!(target_os = "linux").then(|| self.socket())
    }
}

/// A Unix socket's hang-up Linux sets once its peer closed it, or shut
/// both its sides.
impl Hangup for Unsaid<tokio::net::UnixStream> {
    fn hangup_socket(&self) -> Option<std::os::fd::RawFd> {
        use std::os::fd::AsRawFd;
        cfg!(target_os = "linux").then(|| self.0.as_raw_fd())
    }
}

impl Hangup for Unsaid<tokio::io::DuplexStream> {
    fn hangup_socket(&self) -> Option<std::os::fd::RawFd> {
        None
    }
}

/// A TCP peer that closed and one that shut its writing both only send
/// their end (FIN): told apart only by writing to it.
impl Hangup for Unsaid<tokio::net::TcpStream> {
    fn hangup_socket(&self) -> Option<std::os::fd::RawFd> {
        None
    }
}
