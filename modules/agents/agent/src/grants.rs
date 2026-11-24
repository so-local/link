//! `~/.lnk/safe/agents/harness-grants/<harness>.toml`: what a harness said it needs
//! (`lnk-<harness> needs`) and you allowed, and the needs you answered.
//!
//! It's one of two layers. This one is the harness's: asked when an agent
//! switches to it, held only while it runs, never given to another
//! harness, and the same for every agent that runs it here. The other is
//! each agent's ([`crate::sandbox_file`]): what you grant the agent,
//! whichever harness runs it. A harness that needs nothing runs with the
//! agent's alone; `lnk agent harness delete --harness <harness>` deletes its file with it.
//!
//! The file is a sandbox's spec (what you allowed), with `risks`, the
//! harness's own you allowed, and what you answered beside it: `answered`,
//! its risks, and `[asked]`, every need's spec.

use link_format::WithinExt;
use std::collections::BTreeSet;
use std::path::PathBuf;

use link_sandbox_client::Spec;
use link_sandbox_contract::spec::Os;
use serde::{Deserialize, Serialize};

/// What one harness needs here, as you answered. No sandbox at all is
/// never one: it's the agent's own setting (`lnk agent start
/// --no-sandbox`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "toml::Table")]
pub struct Grants {
    /// What it asked for that you allowed, as a spec's sections; none
    /// without a file.
    #[serde(flatten)]
    pub spec: Spec,
    /// The risks of its own it named (`needs`) that you allowed, by its
    /// names for them (`browser`), handed to it as it's configured.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub risks: BTreeSet<String>,
    /// The risks it named that you answered: allowed, or run without,
    /// either way. Not asked again, also once taken back, until `lnk
    /// agent harness reset`.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub answered: BTreeSet<String>,
    /// Every need it asked for that you answered, allowed or not, as one
    /// spec: a need this covers isn't asked again, until `reset`.
    #[serde(default, skip_serializing_if = "is_none")]
    pub asked: Spec,
}

fn is_none(spec: &Spec) -> bool {
    *spec == Spec::default()
}

impl TryFrom<toml::Table> for Grants {
    type Error = anyhow::Error;

    /// A file, or a move's header's, strictly ([`Grants::read`]).
    fn try_from(table: toml::Table) -> anyhow::Result<Self> {
        Grants::read(table)
    }
}

impl Grants {
    /// `table`, a file's or a move's header's, strictly.
    pub fn read(mut table: toml::Table) -> anyhow::Result<Grants> {
        let mut take = |key: &str| -> anyhow::Result<BTreeSet<String>> {
            match table.remove(key) {
                Some(value) => value.try_into().within(key.to_string()),
                None => Ok(BTreeSet::new()),
            }
        };
        let (risks, answered) = (take("risks")?, take("answered")?);
        let asked = match table.remove("asked") {
            Some(asked) => asked.try_into().within("[asked]")?,
            None => Spec::default(),
        };
        let grants = Grants {
            spec: toml::Value::Table(table).try_into()?,
            risks,
            answered,
            asked,
        };
        grants.check()?;
        Ok(grants)
    }
}

/// Why `unsandboxed` is refused where a need or a permission is named:
/// no sandbox is the agent's own setting, never a grant.
pub const NOT_A_GRANT: &str = "no sandbox at all is never a grant: lnk agent start \
     --no-sandbox runs an agent without one, until lnk agent start --sandbox";

/// What a risk a harness names itself opens, beside its own why.
pub const RISK_ABOUT: &str = "a risk the harness names itself";

/// What `key` opens, as a question shows it: a risk the harness names
/// itself, or one thing a spec opens ([`Spec::items`]) by its flag
/// (`--network.mode host`).
pub fn about(key: &str) -> &'static str {
    if link_harness_contract::harness::is_risk_name(key) {
        return RISK_ABOUT;
    }
    // A flag is its key path: its section says what it opens, whichever
    // OS's table it's in.
    let flag = key.split(' ').next().unwrap_or_default();
    let section = flag
        .trim_start_matches("--")
        .split('.')
        .next()
        .unwrap_or_default();
    match (section, flag.rsplit('.').next().unwrap_or_default()) {
        ("network", _) => "this machine's network as it is, with no proxy",
        ("internet", _) => "the internet, through its proxy",
        ("lan", _) => "machines of your network, through its proxy",
        ("localhost", _) => "a port of this machine's",
        ("files", _) => "a folder of this machine's",
        ("tools", _) => "a tool that runs outside its sandbox",
        ("browser", _) => "opening pages in your browser, asking you",
        ("commands", "developer_tools") => "programs it runs",
        ("commands", "toolchains") => "a toolchain's programs, beyond your [commands]",
        ("commands", _) => "a program it starts, beyond your [commands]",
        _ => "",
    }
}

/// `~/.lnk/safe/agents/harness-grants`: a file per harness with grants here.
pub fn dir() -> PathBuf {
    link_files::layout::Hidden::HarnessGrants.path()
}

/// Harness `name`'s file; an error for a name that can't be a harness's
/// (a path, `..`), which never names a file.
pub fn path(name: &str) -> anyhow::Result<PathBuf> {
    check_name(name)?;
    Ok(dir().join(format!("{name}.toml")))
}

/// Whether harness `name` has grants of its own here (a file); never for
/// a name that can't be a harness's.
pub fn exists(name: &str) -> bool {
    path(name).is_ok_and(|p| p.is_file())
}

/// An error unless `name` can name a harness: letters, digits and dashes.
pub fn check_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "{name:?} is not a harness name"
    );
    if let Some(harness) = harness_of_module(name) {
        anyhow::bail!(
            "{name} is the module of harness {harness}: lnk agent harness ... --harness {harness}"
        );
    }
    Ok(())
}

/// The harness whose module `name` is, when it's a module's name and not
/// the harness's (`harness` is `link`'s): no agent runs a harness by
/// that name, so grants under it would hold for none.
pub fn harness_of_module(name: &str) -> Option<&'static str> {
    crate::settings::KNOWN
        .iter()
        .find(|(harness, module, _)| *module == name && *harness != name)
        .map(|(harness, _, _)| *harness)
}

link_files::specification!(Grants, include_str!("grants.schema.toml"));

/// Each harness's grants here, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Grants>()
}

impl Grants {
    /// Harness `name`'s here: none without a file. An error for a file
    /// that isn't one Link wrote: an unknown key, a spec's key that isn't
    /// one, an answer that isn't a risk's.
    pub fn load(name: &str) -> anyhow::Result<Self> {
        let path = path(name)?;
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err).within(format!("reading {}", path.display())),
        };
        link_files::saved::advance::<Grants>(&text)
            .and_then(|text| link_spec::parse::<toml::Table>(&text))
            .and_then(Grants::read)
            .within(format!("reading {}", path.display()))
    }

    /// [`Grants::load`], or, for a file it can't read, none: a start reads
    /// it with [`Grants::load`] first, and fails there.
    pub fn of(name: &str) -> Self {
        Self::load(name).unwrap_or_default()
    }

    /// [`Grants::load`], or for a file it can't read, said on stderr and
    /// taken as none, so `deny`, `ask` and `remove` still work and write it
    /// anew, never wider than it was.
    pub fn load_or_warn(name: &str) -> Self {
        match Self::load(name) {
            Ok(grants) => grants,
            Err(err) => {
                link_format::aside(
                    link_format::Report::warning(link_format::sentence!(
                        "{}'s grants can't be read.",
                        link_format::Name::new("harness", name)
                    ))
                    .why(link_format::Sentence::adopt(&format!(
                        "{err:#}. They are taken as none, and deny and ask write them anew."
                    ))),
                );
                Self::default()
            }
        }
    }

    /// An error for a risk or answered risk that isn't one, a spec that
    /// isn't one on this OS, a list with `"*"` beside other items, or a
    /// limit of calls (the agent's own, never a harness's to lift): what a
    /// file or another machine says is checked before it's kept.
    pub fn check(&self) -> anyhow::Result<()> {
        for risk in self.answered.iter().chain(&self.risks) {
            anyhow::ensure!(
                link_harness_contract::harness::is_risk_name(risk),
                "{:?} isn't a risk's name",
                link_format::clean(risk)
            );
        }
        for spec in [&self.spec, &self.asked] {
            spec.on(Os::here())?;
            spec.check_any_alone()?;
            for os in [Os::Linux, Os::MacOs] {
                anyhow::ensure!(
                    spec.on(os)?.internet.is_none_or(|i| i.per_minute.is_none()),
                    "a harness's grants hold no limit of calls: it's the agent's own, lnk agent \
                     allow --internet.per_minute"
                );
            }
            if let Some(path) = spec.relative_path() {
                anyhow::bail!(
                    "{:?}: a harness's grants name each path absolute, or from ~",
                    link_format::clean(path)
                );
            }
            // Never into the folders every sandbox hides (agents', teams',
            // what's gone): an agent's settings are there, and another's
            // files. One holding them is hidden them all the same.
            let home = link_files::home_dir();
            let hidden = link_sandbox_contract::hidden_dirs();
            for path in spec.paths() {
                let at = match path.strip_prefix("~") {
                    Some(rest) => home.join(rest.trim_start_matches('/')),
                    None => std::path::PathBuf::from(path),
                };
                anyhow::ensure!(
                    !hidden.iter().any(|h| at.starts_with(h)),
                    "{:?}: a harness's grants never reach the Link folder's agents, teams or \
                     history",
                    link_format::clean(path)
                );
            }
        }
        Ok(())
    }

    /// Writes them as harness `name`'s, only the user's.
    pub fn save(&self, name: &str) -> anyhow::Result<()> {
        let path = path(name)?;
        let text = format!(
            "# What {name} said it needs on this machine and you allowed, for every agent \
             that runs it: a sandbox's spec, its risks, and what you answered.\n# lnk agent \
             harness permissions --harness {name} shows it; lnk agent harness-permission remove --harness \
             {name} with a spec's flags, or a risk, takes one back.\n\n{}",
            link_files::saved::print(self)?
        );
        link_files::write_private(&path, text.as_bytes())
            .within(format!("writing {}", path.display()))
    }

    /// Deletes harness `name`'s file; whether there was one.
    pub fn remove(name: &str) -> anyhow::Result<bool> {
        let path = path(name)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err).within(format!("deleting {}", path.display())),
        }
    }

    /// Of what these allow here, each thing `agent` (the agent's own
    /// spec) doesn't open already.
    fn items_beyond(&self, agent: &Spec) -> Vec<Spec> {
        let here = self.spec.on(Os::here()).unwrap_or_default();
        here.items()
            .into_iter()
            .filter(|item| !agent.covers(item, Os::here()).unwrap_or(false))
            .collect()
    }

    /// What it runs with beyond `agent`'s (the agent's own spec), each as
    /// `lnk agent harness-permission remove` takes it back: a spec's flags
    /// (`--network.mode host`), a risk of its own (`browser`).
    pub fn beyond(&self, agent: &Spec) -> Vec<String> {
        let mut beyond: Vec<String> = self
            .items_beyond(agent)
            .iter()
            .map(link_sandbox_client::flags::shown)
            .collect();
        beyond.extend(self.risks.iter().cloned());
        beyond
    }

    /// What these open beyond `from`'s, both beside the agent's own spec
    /// (`agent`), each as `deny` names it with what it opens: for
    /// switching an agent from a harness with `from` to one with these,
    /// which the user agrees to first.
    pub fn wider_than(&self, from: &Grants, agent: &Spec) -> Vec<(String, String)> {
        let had = agent.join(&from.spec);
        let mut wider: Vec<(String, String)> = self
            .items_beyond(&had)
            .iter()
            .map(|item| {
                let key = link_sandbox_client::flags::shown(item);
                let about = about(&key).to_string();
                (key, about)
            })
            .collect();
        for risk in self.risks.difference(&from.risks) {
            wider.push((risk.clone(), RISK_ABOUT.into()));
        }
        wider
    }

    /// What `from` opened that these don't, beside the agent's own spec
    /// (`agent`), by the names `deny` takes.
    pub fn narrower_than(&self, from: &Grants, agent: &Spec) -> Vec<String> {
        let has = agent.join(&self.spec);
        let mut narrower: Vec<String> = from
            .items_beyond(&has)
            .iter()
            .map(link_sandbox_client::flags::shown)
            .collect();
        narrower.extend(from.risks.difference(&self.risks).cloned());
        narrower
    }

    /// These and `other`'s: every grant, risk and answer of both.
    pub fn union(&mut self, other: &Grants) {
        self.spec = self.spec.join(&other.spec);
        self.risks.extend(other.risks.iter().cloned());
        self.answered.extend(other.answered.iter().cloned());
        self.asked = self.asked.join(&other.asked);
    }

    /// Of these, only what `needs` (what the harness says it needs here)
    /// asks for: what its specs open, its risks, and the answers to
    /// those. What another machine gave the harness is kept only as far
    /// as it asks for it here; a need it doesn't name, or an answer it
    /// can't be read for, gives nothing.
    pub fn only_asked(&self, needs: &link_harness_contract::harness::Needs) -> Grants {
        let Ok(specs) = needs.checked() else {
            return Grants::default();
        };
        let asked = specs
            .iter()
            .fold(Spec::default(), |all, (spec, _)| all.join(spec));
        let only = |spec: &Spec| {
            let here = spec.on(Os::here()).unwrap_or_default();
            let lost = here
                .items()
                .into_iter()
                .filter(|item| !asked.covers(item, Os::here()).unwrap_or(false))
                .fold(Spec::default(), |all, item| all.join(&item));
            here.without(&lost)
        };
        let risks: BTreeSet<&str> = needs.risks.iter().map(|r| r.name.as_str()).collect();
        Grants {
            spec: only(&self.spec),
            risks: self
                .risks
                .iter()
                .filter(|r| risks.contains(r.as_str()))
                .cloned()
                .collect(),
            answered: self
                .answered
                .iter()
                .filter(|a| risks.contains(a.as_str()))
                .cloned()
                .collect(),
            asked: only(&self.asked),
        }
    }

    /// What it has, in a line: `--network.mode host, browser`, or
    /// `nothing`.
    pub fn summary(&self) -> String {
        let granted = self.beyond(&Spec::default());
        match granted.is_empty() {
            true => "nothing".to_string(),
            false => granted.join(", "),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(text: &str) -> Spec {
        link_spec::parse(text).unwrap()
    }

    fn read_grants(text: &str) -> Grants {
        Grants::read(link_spec::parse(text).unwrap()).unwrap()
    }

    /// A harness's grants never reach into the folders every sandbox
    /// hides (an agent's settings are in its folder), but may hold them.
    #[test]
    fn a_harness_never_reaches_an_agents_folder() {
        // HOME as every test of an agent's files sets it, before it's read
        // twice below: once for `~`, once for the folders hidden.
        let _home = crate::settings::tests::own_config();
        let grants = |text: &str| Grants {
            spec: spec(text),
            ..Grants::default()
        };
        for inside in [
            "~/Link/Agents/main",
            "~/Link/Safe/Agents/other/agent.toml",
            "~/Link/Archive",
        ] {
            let error = grants(&format!("[files]\nread = [\"{inside}\"]\n"))
                .check()
                .unwrap_err();
            assert!(
                error.to_string().contains("never reach"),
                "{inside}: {error}"
            );
        }
        grants("[files]\nread = [\"~\", \"~/Link/Files\"]\n")
            .check()
            .unwrap();
    }

    #[test]
    fn the_union_is_both() {
        let mut a = Grants {
            spec: spec("[network]\nmode = \"host\"\n"),
            ..Grants::default()
        };
        let b = Grants {
            risks: ["browser".to_string()].into(),
            ..Grants::default()
        };
        a.union(&b);
        assert_eq!(
            a.beyond(&Spec::default()),
            ["--network.mode host", "browser"]
        );
        // What the agent has already isn't beyond it.
        assert_eq!(a.beyond(&spec("[network]\nmode = \"host\"\n")), ["browser"]);
    }

    #[test]
    fn only_what_the_harness_asks_for_is_kept() {
        use link_harness_contract::harness::{Need, Needs, Risk};
        let given = Grants {
            spec: spec("[network]\nmode = \"host\"\n[localhost]\nports = [8080]\n"),
            risks: ["browser".to_string(), "signal-relay".to_string()].into(),
            answered: ["browser", "signal-relay"].map(String::from).into(),
            asked: spec("[network]\nmode = \"host\"\n[localhost]\nports = [8080]\n"),
        };
        let needs = Needs {
            needs: vec![Need {
                spec: spec("[network]\nmode = \"host\"\n"),
                why: "its sockets ignore the proxy".into(),
            }],
            risks: vec![Risk {
                name: "browser".into(),
                why: "a port of its own".into(),
                without: Some("its browser stays off".into()),
            }],
            ..Needs::default()
        };
        let kept = given.only_asked(&needs);
        assert_eq!(kept.spec, spec("[network]\nmode = \"host\"\n"));
        assert_eq!(kept.asked, kept.spec);
        assert_eq!(kept.risks, ["browser".to_string()].into());
        assert_eq!(kept.answered, ["browser"].map(String::from).into());
        // A harness that needs nothing, or an answer that isn't one, gets nothing.
        assert_eq!(given.only_asked(&Needs::default()), Grants::default());
        let bad = Needs {
            needs: vec![Need {
                spec: spec("[network]\nmode = \"host\"\n"),
                why: String::new(),
            }],
            ..Needs::default()
        };
        assert_eq!(given.only_asked(&bad), Grants::default());
    }

    #[test]
    fn only_a_harness_name_names_a_file_and_only_known_answers_are_kept() {
        for bad in ["", "../agents/main", "a/b", "a.b"] {
            assert!(path(bad).is_err(), "{bad:?}");
            assert!(!exists(bad), "{bad:?}");
        }
        assert!(
            path("link")
                .unwrap()
                .ends_with("safe/agents/harness-grants/link.toml")
        );
        // A module's name, never its harness's file.
        assert!(path("harness").is_err());
        assert_eq!(harness_of_module("harness"), Some("link"));
        assert_eq!(harness_of_module("link"), None);
        assert_eq!(harness_of_module("mini-harness"), Some("mini"));
        assert_eq!(harness_of_module("mini"), None);
        let mut grants = Grants::default();
        // A harness's own name for a risk.
        grants.answered.insert("signal-relay".into());
        grants.risks.insert("signal-relay".into());
        grants.check().unwrap();
        grants.answered.insert("no such need".into());
        assert!(grants.check().is_err());
        let bad_risk = Grants {
            risks: ["tool".to_string()].into(),
            ..Grants::default()
        };
        assert!(bad_risk.check().is_err());
        // Not a spec's key, and no relative path.
        for bad in ["hosts = [\"a.com\"]\n", "[files]\nread = [\"./x\"]\n"] {
            let table: toml::Table = toml::from_str(bad).unwrap();
            assert!(Grants::read(table).is_err(), "{bad}");
        }
        // What it writes, it reads back.
        let kept = Grants {
            spec: spec("[lan]\nhosts = [\"*\"]\n"),
            risks: ["browser".to_string()].into(),
            answered: ["browser".to_string()].into(),
            asked: spec("[lan]\nhosts = [\"*\"]\n[browser]\nhosts = [\"*\"]\n"),
        };
        let text = link_spec::print(&kept).unwrap();
        assert_eq!(read_grants(&text), kept);
    }

    #[test]
    fn a_switch_says_what_it_opens_and_what_it_closes() {
        let agent = spec("[internet]\nhosts = [\"*\"]\n[files]\nread = [\"~\"]\n");
        let link = Grants::default();
        let other = Grants {
            spec: spec("[network]\nmode = \"host\"\n[files]\nread = [\"~\"]\n"),
            risks: ["browser".to_string()].into(),
            ..Grants::default()
        };
        let wider: Vec<String> = other
            .wider_than(&link, &agent)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        // `~` is the agent's already: nothing new.
        assert_eq!(wider, ["--network.mode host", "browser"]);
        assert_eq!(
            link.narrower_than(&other, &agent),
            ["--network.mode host", "browser"]
        );
        assert!(other.narrower_than(&link, &agent).is_empty());
        // The same grants: nothing either way.
        assert!(other.wider_than(&other, &agent).is_empty());
        assert_eq!(
            about("--network.mode host"),
            "this machine's network as it is, with no proxy"
        );
        assert_eq!(about("browser"), RISK_ABOUT);
    }
}
