//! What a call on the sandbox's own routes may reach, as its bridge
//! decides it at each call: never from the route's `given` alone, which
//! was fixed as its proxy started, but from what the caller is granted
//! now. A person's own run (`lnk sandbox run`, bound as `run-<name>`)
//! reaches what its routes say, as that person chose them. An agent
//! reaches what the module that keeps it says it's granted at that call
//! ([`GRANTS`]), or, where what keeps it is no module (a service that
//! binds agents into sandboxes), what that keeper wrote for it in the
//! bridge's settings ([`keep`]); with no keeper's answer it reaches
//! nothing.

use std::path::{Path, PathBuf};

use link_core::answers::{Answer, Question};
use serde::{Deserialize, Serialize};

/// Where a person's own run's binding names it: `run-<name>`, never an
/// agent's ID, which is lowercase letters and digits alone.
pub const RUN_PREFIX: &str = "run-";

/// The sandbox's bridge's question, at each call, to the modules that
/// keep agents: what agent [`Asked::agent`] may reach now.
pub const GRANTS: Question<Asked, Granted> =
    Question::new("sandbox.grants", include_str!("grants.toml"));

/// What [`GRANTS`] asks: the agent a call is proved as, by its ID, and
/// the sandbox it calls from, `<agent>-<harness>` on its wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asked {
    pub agent: String,
    pub sandbox: String,
}

/// A module's answer to [`GRANTS`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Granted {
    /// It keeps the agent, which may now reach these.
    Agent(Current),
    /// It keeps no agent of that ID.
    NotMine,
}

/// What an agent may reach now on the sandbox's own routes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Current {
    /// The tools it may call.
    #[serde(default)]
    pub tools: Vec<String>,
    /// The hosts a page it opens may be on (`["*"]`, any); none, it
    /// opens no page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<Vec<String>>,
}

link_files::specification!(Current, include_str!("grants.schema.toml"));

/// Whose word a call's reach is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Authority {
    /// A person's own run: its routes are theirs.
    Run,
    /// An agent, as its keeper answered at this call.
    Agent(Current),
}

impl Authority {
    /// The authority of a call proved as `agent` from `sandbox`, asked
    /// of the modules that keep agents when it's no run.
    pub fn of(agent: &str, sandbox: &str) -> anyhow::Result<Authority> {
        if agent.starts_with(RUN_PREFIX) {
            return Ok(Authority::Run);
        }
        let asked = Asked {
            agent: agent.to_string(),
            sandbox: sandbox.to_string(),
        };
        let kept = || match link_files::layout::Hidden::SandboxGrants.try_path() {
            Ok(settings) => kept(&settings, agent),
            Err(_) => Ok(None),
        };
        Authority::answered(agent, GRANTS.ask(&asked), kept)
    }

    /// The authority the keepers' `answers` say agent `agent` has: the
    /// first that keeps it; an error when none does, or one failed, so a
    /// keeper that couldn't answer never lets more through.
    /// With no module keeping it, what `kept` says a keeper that's no
    /// module wrote for it ([`keep`]).
    pub fn answered(
        agent: &str,
        answers: impl IntoIterator<Item = Answer<Granted>>,
        kept: impl FnOnce() -> anyhow::Result<Option<Current>>,
    ) -> anyhow::Result<Authority> {
        let mut failed = None;
        for answer in answers {
            match answer.reply {
                Ok(Granted::Agent(current)) => return Ok(Authority::Agent(current)),
                Ok(Granted::NotMine) => {}
                Err(err) => failed = Some((answer.module, err)),
            }
        }
        if let Some((module, err)) = failed {
            return Err(err.context(format!(
                "{module} couldn't say what agent {} may reach",
                link_format::clean(agent)
            )));
        }
        match kept()? {
            Some(current) => Ok(Authority::Agent(current)),
            None => anyhow::bail!(
                "no keeper here keeps agent {}, so it reaches nothing",
                link_format::clean(agent)
            ),
        }
    }

    /// Whether it may call tool `name`, its route naming `named`.
    pub fn tool(&self, name: &str, named: &[String]) -> bool {
        let listed = named.iter().any(|t| t == name);
        match self {
            Authority::Run => listed,
            Authority::Agent(current) => listed && current.tools.iter().any(|t| t == name),
        }
    }

    /// The hosts a page may be on, of the route's `given` (`["*"]`, any)
    /// and, for an agent, its current grant: none when it may open none.
    pub fn page_hosts(&self, given: &[String]) -> Option<Vec<Vec<String>>> {
        match self {
            Authority::Run => Some(vec![given.to_vec()]),
            Authority::Agent(current) => current
                .browser
                .as_ref()
                .map(|now| vec![given.to_vec(), now.clone()]),
        }
    }
}

/// Where a keeper that's no module keeps what agent `agent` (its ID)
/// may reach, in the sandbox state folder `grants` of the settings its
/// bridges serve (Hidden's `sandbox-grants`): an error for an ID that
/// isn't one, lowercase letters and digits alone.
fn kept_in(grants: &Path, agent: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !agent.is_empty()
            && agent
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
        "{} isn't an agent's ID",
        link_format::clean(agent)
    );
    Ok(grants.join(format!("{agent}.json")))
}

/// The file [`keep`] keeps agent `agent`'s grant in, for the bridges of
/// Link's settings `settings`: for a keeper that writes it as another
/// user than theirs, and gives it to them, since [`keep`] makes the
/// folder its own.
pub fn kept_file(settings: &Path, agent: &str) -> anyhow::Result<PathBuf> {
    kept_in(
        &link_files::layout::Hidden::SandboxGrants.at(settings),
        agent,
    )
}

/// Keeps what agent `agent` may reach now, for the bridges of Link's
/// settings `settings`: a keeper that's no module (a service binding
/// agents into sandboxes) writes it when it binds the agent, and
/// [`forget`]s it when it lets the agent go. Its file, which the caller
/// gives to the bridges' user where that's another.
pub fn keep(settings: &Path, agent: &str, current: &Current) -> anyhow::Result<PathBuf> {
    let grants = link_files::layout::Hidden::SandboxGrants.at(settings);
    let file = kept_in(&grants, agent)?;
    link_files::create_private_dir(&grants)?;
    link_files::write_private(&file, &link_files::saved::print_json(current)?)?;
    Ok(file)
}

/// Forgets what [`keep`] kept for agent `agent`: it reaches nothing more.
pub fn forget(settings: &Path, agent: &str) -> anyhow::Result<()> {
    let grants = link_files::layout::Hidden::SandboxGrants.at(settings);
    match std::fs::remove_file(kept_in(&grants, agent)?) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
        _ => Ok(()),
    }
}

/// What [`keep`] kept for agent `agent` in the folder `grants`: none
/// when nothing is kept for it.
fn kept(grants: &Path, agent: &str) -> anyhow::Result<Option<Current>> {
    let file = kept_in(grants, agent)?;
    match std::fs::read(&file) {
        Ok(bytes) => Ok(Some(link_files::saved::parse_json(&bytes)?)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// Whether `host` is on every list of `lists` ([`Authority::page_hosts`]),
/// each `["*"]` for any.
pub fn page_allowed(host: &str, lists: &[Vec<String>]) -> bool {
    lists.iter().all(|hosts| {
        hosts.iter().any(|h| h == "*") || link_sandbox_contract::net::host_allowed(host, hosts)
    })
}

#[cfg(test)]
mod tests {

    /// The question this owns declares its protocol beside it.

    #[test]

    fn its_question_declares_its_protocol() {
        let protocol = GRANTS.protocol();

        assert!(protocol.oldest <= protocol.current, "{protocol:?}");
    }
    use super::*;

    fn answer(module: &str, reply: anyhow::Result<Granted>) -> Answer<Granted> {
        Answer {
            module: module.into(),
            reply,
        }
    }

    fn agent(tools: &[&str], browser: Option<&[&str]>) -> Granted {
        Granted::Agent(Current {
            tools: tools.iter().map(|t| t.to_string()).collect(),
            browser: browser.map(|b| b.iter().map(|h| h.to_string()).collect()),
        })
    }

    fn nothing_kept() -> anyhow::Result<Option<Current>> {
        Ok(None)
    }

    /// Kept by a keeper that's no module: what it kept, where no module
    /// keeps the agent; nothing once it's forgotten; never another ID's,
    /// or a path in place of one.
    #[test]
    fn a_keeper_that_is_no_module_answers_with_what_it_kept() {
        let root = link_test_kit::TempDir::new("grants");
        let settings = root.join(".lnk");
        let id = "a1b2c3d4e5f6";
        keep(
            &settings,
            id,
            &Current {
                tools: list(&["file-a1b2c3d4e5f6"]),
                browser: None,
            },
        )
        .unwrap();
        let grants = link_files::layout::Hidden::SandboxGrants.at(&settings);
        let now = Authority::answered(id, [answer("lnk-x", Ok(Granted::NotMine))], || {
            kept(&grants, id)
        })
        .unwrap();
        assert!(now.tool("file-a1b2c3d4e5f6", &list(&["file-a1b2c3d4e5f6"])));
        assert!(kept(&grants, "b1b2c3d4e5f6").unwrap().is_none());
        assert!(keep(&settings, "../x", &Current::default()).is_err());
        forget(&settings, id).unwrap();
        forget(&settings, id).unwrap();
        let gone = Authority::answered(id, [], || kept(&grants, id));
        assert!(gone.unwrap_err().to_string().contains("reaches nothing"));
    }

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|i| i.to_string()).collect()
    }

    #[test]
    fn a_run_reaches_what_its_routes_say() {
        let run = Authority::of("run-notes", "notes").unwrap();
        assert_eq!(run, Authority::Run);
        assert!(run.tool("gh", &list(&["gh"])));
        assert!(!run.tool("aws", &list(&["gh"])));
        let lists = run.page_hosts(&list(&["*"])).unwrap();
        assert!(page_allowed("example.com", &lists));
    }

    #[test]
    fn an_agent_reaches_what_its_keeper_grants_now() {
        let now = Authority::answered(
            "a1b2c3d4e5f6",
            [
                answer("lnk-other", Ok(Granted::NotMine)),
                answer("lnk-agent", Ok(agent(&["gh"], Some(&["*.example.com"])))),
            ],
            nothing_kept,
        )
        .unwrap();
        // Named by its route and granted now.
        assert!(now.tool("gh", &list(&["gh", "aws"])));
        // Named by its route as its proxy started, but taken back since.
        assert!(!now.tool("aws", &list(&["gh", "aws"])));
        // Granted, but not its route's: the route is never widened.
        assert!(!now.tool("gh", &list(&["aws"])));
        let lists = now.page_hosts(&list(&["*"])).unwrap();
        assert!(page_allowed("docs.example.com", &lists));
        assert!(!page_allowed("evil.test", &lists));
        let none = Authority::Agent(Current::default());
        assert_eq!(none.page_hosts(&list(&["*"])), None);
    }

    #[test]
    fn no_keeper_or_a_failed_one_reaches_nothing() {
        let unkept = Authority::answered(
            "a1b2c3d4e5f6",
            [answer("lnk-x", Ok(Granted::NotMine))],
            nothing_kept,
        );
        assert!(unkept.unwrap_err().to_string().contains("reaches nothing"));
        let none = Authority::answered("a1b2c3d4e5f6", [], nothing_kept);
        assert!(none.is_err());
        let failed = Authority::answered(
            "a1b2c3d4e5f6",
            [answer(
                "lnk-agent",
                Err(anyhow::anyhow!("settings unreadable")),
            )],
            || Ok(Some(Current::default())),
        );
        assert!(failed.unwrap_err().to_string().contains("lnk-agent"));
    }

    #[test]
    fn its_answer_is_strict_json() {
        let text = r#"{"agent":{"tools":["gh"],"browser":["*"]}}"#;
        let granted: Granted = serde_json::from_str(text).unwrap();
        assert_eq!(granted, agent(&["gh"], Some(&["*"])));
        assert!(serde_json::from_str::<Granted>(r#"{"agent":{"tools":[],"policy":"x"}}"#).is_err());
        assert_eq!(
            serde_json::to_string(&Granted::NotMine).unwrap(),
            r#""not_mine""#
        );
    }
}
