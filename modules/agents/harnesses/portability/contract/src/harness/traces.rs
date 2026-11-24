//! What a harness keeps outside its home when it runs outside Link, as
//! data: each place, and what kind of thing is there. An adapter declares
//! the list (`lnk-<name> traces`); finding an install, migrating it,
//! `lnk agent harness show` and `lnk agent delete` all read the one list,
//! so any adapter, from any repository, is found, migrated and cleaned the
//! same way.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One place a harness writes when it isn't boxed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trace {
    pub kind: Kind,
    /// `~/` for the home folder and `{uid}` for the user's ID, else an
    /// absolute path: `~/.openclaw`, `/tmp/openclaw-state-locks-{uid}`.
    pub path: Cow<'static, str>,
}

/// What kind of thing is at a [`Trace`]: what a migration does with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Its home, holding all its state: copied whole.
    Home,
    /// Its settings file, in its home or not.
    Config,
    /// A file of its secrets (keys, bot tokens), `.env`-style: each goes
    /// into Link's secrets.
    Secrets,
    /// A login of its own that can't move (an OAuth session): said, and
    /// left working.
    Login,
    /// Its own service (a systemd unit, a launchd plist): stopped and
    /// disabled before Link runs it, never deleted until cleaned.
    Service,
    /// A program it installed (a launcher on the `PATH`).
    Program,
    /// What its installer put down beside it (a checkout, a global
    /// package).
    Package,
    /// Caches, made again when needed.
    Cache,
    /// Its logs.
    Log,
    /// Locks and runtime files of a running harness.
    Lock,
    /// A browser profile it drives.
    Browser,
    /// One of several agents sharing its home: `{agent}` in the path is
    /// the agent's name, `{-agent}` a dash and its name; for the install's
    /// own agent, [`OWN`] and nothing. Each becomes an agent of Link's,
    /// its part landing where the install's own agent's is.
    Agent,
    /// Another agent's whole home, `{agent}` its name (a profile): each
    /// becomes an agent of Link's, the folder landing as its home.
    Profile,
}

/// The name of an install's own agent, in a [`Kind::Agent`] path.
pub const OWN: &str = "main";

/// The version that wrote an install's state, as its adapter reads it
/// (`lnk-<name> written <its home>`), and why Link can't run that state,
/// when it can't: a state Link can't read is refused before anything is
/// copied.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Written {
    /// As the harness writes it (`2026.9.6`, `46`); none when its state
    /// holds none.
    pub version: Option<String>,
    /// Why Link can't run it, in words.
    pub refused: Option<String>,
}

/// Whether dotted version `a` (`2026.10.1`, `47`) is newer than `b`,
/// part by part as numbers; a part that isn't a number compares as 0.
pub fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parts(a) > parts(b)
}

impl Kind {
    /// How `lnk agent harness show` labels it.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Home => "home",
            Kind::Config => "settings",
            Kind::Secrets => "secrets",
            Kind::Login => "login",
            Kind::Service => "service",
            Kind::Program => "program",
            Kind::Package => "install",
            Kind::Cache => "cache",
            Kind::Log => "logs",
            Kind::Lock => "locks",
            Kind::Browser => "browser",
            Kind::Agent => "agent",
            Kind::Profile => "profile",
        }
    }
}

impl Trace {
    /// A trace an adapter declares, in a `const`.
    pub const fn new(kind: Kind, path: &'static str) -> Self {
        Self {
            kind,
            path: Cow::Borrowed(path),
        }
    }

    /// Where it is for the user whose home folder is `home` and whose ID
    /// is `uid`.
    pub fn at(&self, home: &Path, uid: &str) -> PathBuf {
        self.of(home, uid, None)
    }

    /// Where it is for agent `agent` of the install ([`Kind::Agent`],
    /// [`Kind::Profile`]); none: the install's own.
    pub fn of(&self, home: &Path, uid: &str, agent: Option<&str>) -> PathBuf {
        let path = self
            .path
            .replace("{uid}", uid)
            .replace(
                "{-agent}",
                &agent.map(|a| format!("-{a}")).unwrap_or_default(),
            )
            .replace("{agent}", agent.unwrap_or(OWN));
        match path.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trace_is_typed_on_the_wire_and_the_unexpected_refused() {
        let traces = [
            Trace::new(Kind::Home, "~/.otter"),
            Trace::new(Kind::Lock, "/tmp/otter-{uid}"),
        ];
        let json = serde_json::to_string(&traces).unwrap();
        assert_eq!(
            json,
            r#"[{"kind":"home","path":"~/.otter"},{"kind":"lock","path":"/tmp/otter-{uid}"}]"#
        );
        assert_eq!(serde_json::from_str::<Vec<Trace>>(&json).unwrap(), traces);
        assert!(serde_json::from_str::<Vec<Trace>>(r#"["~/.otter"]"#).is_err());
        assert!(serde_json::from_str::<Vec<Trace>>(r#"[{"kind":"other","path":"~/x"}]"#).is_err());
        let home = Path::new("/home/me");
        assert_eq!(traces[0].at(home, "501"), Path::new("/home/me/.otter"));
        assert_eq!(traces[1].at(home, "501"), Path::new("/tmp/otter-501"));
    }

    /// An agent's place is its name in the path, the install's own
    /// agent's `main` and no dash.
    #[test]
    fn an_agents_place_names_it() {
        let home = Path::new("/home/me");
        let part = Trace::new(Kind::Agent, "~/.otter/agents/{agent}");
        let work = Trace::new(Kind::Agent, "~/.otter/work{-agent}");
        assert_eq!(
            part.of(home, "1", Some("kit")),
            Path::new("/home/me/.otter/agents/kit")
        );
        assert_eq!(
            part.of(home, "1", None),
            Path::new("/home/me/.otter/agents/main")
        );
        assert_eq!(
            work.of(home, "1", Some("kit")),
            Path::new("/home/me/.otter/work-kit")
        );
        assert_eq!(work.of(home, "1", None), Path::new("/home/me/.otter/work"));
        assert!(newer("2026.10.1", "2026.9.6"));
        assert!(!newer("2026.9.6", "2026.9.6"));
        assert!(newer("47", "46") && !newer("9", "46"));
    }
}
