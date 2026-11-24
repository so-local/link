//! Agents' IDs here, `agent:<id>`, the services they act as,
//! `service:<id>`, and teams' IDs, `group:<id>`: each minted once, when
//! the agent or the team is made, and never given again. An agent's service has an ID of its own, never the
//! agent's, since a service is who acts and the agent what runs. Each
//! has its ledger (`~/.lnk/safe/agents/ids.toml`, `services.toml`,
//! `team-ids.toml`), which keeps every ID minted or brought here and every
//! ID retired by `lnk agent delete` or `lnk agent team delete`, under its
//! lock; an ID another agent or team here has is live too, read from its
//! settings or its team file. A name is a label: no lookup tells a
//! name from an ID by its shape.

use std::collections::BTreeSet;

use link_format::WithinExt;
use link_id::Ledger;

/// The kind an agent's ID is written with.
pub const KIND: &str = "agent";

/// The kind an agent's service is written with: a principal's.
pub const SERVICE: &str = "service";

/// `id` as it's written, `agent:<id>`.
pub fn written(id: &str) -> String {
    AGENTS.written(id)
}

/// The agent whose ID is `id`, bare, as its type writes it: `agent:<id>`.
pub fn workload(id: &str) -> anyhow::Result<link_id::Workload> {
    link_machine::machine::check_id(id)?;
    Ok(link_id::Workload::of(KIND, id)?)
}

/// The ID `text` writes, `agent:<id>`: refused bare, or of another kind.
pub fn parse(text: &str) -> anyhow::Result<String> {
    AGENTS.parse(text)
}

/// Where a ledger reads the IDs of what's here now.
#[derive(Clone, Copy)]
enum Present {
    /// Each agent's identity, by its field.
    Identity(&'static str),
    /// Each team's file.
    Teams,
}

/// One kind's ledger here: the IDs of what's here now are read as its
/// `present` says.
#[derive(Clone, Copy)]
pub struct Book {
    kind: &'static str,
    what: &'static str,
    present: Present,
    file: File,
}

/// Where a ledger's file is.
#[derive(Clone, Copy)]
enum File {
    /// A location of the layout's own.
    Hidden(link_files::layout::Hidden),
    /// A file of this name in the agents' folder (`~/.lnk/safe/agents`).
    InAgents(&'static str),
}

impl File {
    fn path(self) -> anyhow::Result<std::path::PathBuf> {
        Ok(match self {
            File::Hidden(hidden) => hidden.try_path()?,
            File::InAgents(name) => link_files::layout::Hidden::Agents.try_path()?.join(name),
        })
    }
}

/// Agents' IDs.
pub const AGENTS: Book = Book {
    kind: KIND,
    what: "an agent's ID",
    present: Present::Identity("id"),
    file: File::Hidden(link_files::layout::Hidden::AgentIds),
};

/// The services agents here act as.
pub const SERVICES: Book = Book {
    kind: SERVICE,
    what: "an agent's service",
    present: Present::Identity("service"),
    file: File::Hidden(link_files::layout::Hidden::AgentServices),
};

/// The file of teams' IDs in the agents' folder.
pub const TEAM_IDS: &str = "team-ids.toml";

/// Teams' IDs, each a group's, `group:<id>`.
pub const TEAMS: Book = Book {
    kind: link_agent_team::KIND,
    what: "a team's ID",
    present: Present::Teams,
    file: File::InAgents(TEAM_IDS),
};

/// A ledger as its file keeps it, naming its schema.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Kept(link_id::Table);

link_files::specification!(Kept, include_str!("ids.schema.toml"));

/// The ledger here: what's kept, and the IDs of the agents here now.
struct Here {
    kept: link_id::Table,
    present: BTreeSet<String>,
}

impl Ledger for Here {
    fn is_live(&self, id: &str) -> bool {
        self.kept.is_live(id) || self.present.contains(id)
    }
    fn is_retired(&self, id: &str) -> bool {
        self.kept.is_retired(id)
    }
    fn hold(&mut self, id: &str) {
        self.kept.hold(id);
    }
    fn retire(&mut self, id: &str) {
        self.kept.retire(id);
    }
}

impl Book {
    /// `id` as it's written, `<kind>:<id>`.
    pub fn written(&self, id: &str) -> String {
        format!("{}:{id}", self.kind)
    }

    /// The ID `text` writes, `<kind>:<id>`: refused bare, or of another
    /// kind.
    pub fn parse(&self, text: &str) -> anyhow::Result<String> {
        let id = text
            .strip_prefix(self.kind)
            .and_then(|rest| rest.strip_prefix(':'))
            .filter(|id| link_id::is_id(id))
            .within(format!(
                "{:?} isn't {}: {}:<its id>",
                link_format::clean(text),
                self.what,
                self.kind
            ))?;
        Ok(id.to_string())
    }

    /// The ID agent `name`'s identity here says, read alone, or team
    /// `name`'s file.
    pub fn of(&self, name: &str) -> Option<String> {
        link_machine::machine::check_shape(name).ok()?;
        match self.present {
            Present::Identity(field) => {
                let identity = super::records::identity(name).ok()??;
                match field {
                    "id" => Some(identity.id),
                    _ => identity.service,
                }
            }
            Present::Teams => super::teams()
                .ok()?
                .into_iter()
                .find(|team| team.name == name)
                .map(|team| team.id),
        }
    }

    /// `change` on the ledger here, under its lock, kept when it
    /// succeeds.
    fn with<T>(&self, change: impl FnOnce(&mut Here) -> anyhow::Result<T>) -> anyhow::Result<T> {
        let path = self.file.path()?;
        with_at(&path, || self.present(), change)
    }

    /// The IDs of the agents or teams here now, from their settings or
    /// their files.
    fn present(&self) -> anyhow::Result<BTreeSet<String>> {
        Ok(match self.present {
            Present::Identity(_) => super::names()?
                .iter()
                .filter_map(|name| self.of(name))
                .collect(),
            Present::Teams => super::teams()?.into_iter().map(|team| team.id).collect(),
        })
    }

    /// A new ID, reserved here: never one an agent here has, had or was
    /// given.
    pub fn reserve(&self) -> anyhow::Result<String> {
        self.with(|here| Ok(link_id::reserve(here)?))
    }

    /// Takes `id` for an agent arriving here (a move, a restore): refused
    /// when it's retired here, or when another agent here has it.
    pub fn admit(&self, id: &str) -> anyhow::Result<()> {
        self.with(|here| take(here, id))
            .within(format!("taking {}", self.written(id)))
    }

    /// Reserves `id`, which a definition gives what it makes: refused
    /// while it's live here, held in the ledger or had by one here (a
    /// team moved to a hosted account, or taken by an apply at once), and
    /// once it's retired.
    pub fn claim(&self, id: &str) -> anyhow::Result<()> {
        self.with(|here| Ok(link_id::admit(here, id)?))
            .within(format!("taking {}", self.written(id)))
    }

    /// Takes `id` for agent `name` arriving by a move: nothing to take
    /// when `name` here already has it (a move retried), else
    /// [`Book::admit`].
    pub fn arrive(&self, name: &str, id: &str) -> anyhow::Result<()> {
        match self.of(name).as_deref() == Some(id) {
            true => Ok(()),
            false => self.admit(id),
        }
    }

    /// Takes `id` back for the agent `lnk agent restore` brings back from
    /// its buckets: the one thing the ID ever named, so the one way a
    /// retired ID returns. Its secrets went with its delete, so no grant
    /// of before reaches it. Refused while another agent here has it.
    pub fn restore(&self, id: &str) -> anyhow::Result<()> {
        self.with(|here| revive(here, id))
            .within(format!("taking {} back", self.written(id)))
    }

    /// Retires `id`, of an agent deleted here, for good.
    pub fn retire(&self, id: &str) -> anyhow::Result<()> {
        self.with(|here| {
            here.retire(id);
            Ok(())
        })
    }

    /// Whether `id` was retired here.
    pub fn is_retired(&self, id: &str) -> anyhow::Result<bool> {
        self.with(|here| Ok(here.is_retired(id)))
    }
}

/// `change` on the ledger at `path`, under its lock, with the agents
/// `present` reads, kept when it succeeds.
fn with_at<T>(
    path: &std::path::Path,
    present: impl FnOnce() -> anyhow::Result<BTreeSet<String>>,
    change: impl FnOnce(&mut Here) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let _lock = link_files::lock_file(&path.with_extension("lock"))?;
    let kept = match std::fs::read_to_string(path) {
        Ok(text) => {
            link_files::saved::parse::<Kept>(&text)
                .within(format!("reading {}", path.display()))?
                .0
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => link_id::Table::default(),
        Err(err) => return Err(err).within(format!("reading {}", path.display())),
    };
    let mut here = Here {
        kept,
        present: present()?,
    };
    let before = here.kept.clone();
    let answer = change(&mut here)?;
    if here.kept != before {
        link_files::saved::write(path, &Kept(here.kept.clone()))
            .within(format!("writing {}", path.display()))?;
    }
    Ok(answer)
}

fn take(here: &mut Here, id: &str) -> anyhow::Result<()> {
    if here.is_retired(id) {
        Err(link_id::Refused::Retired)?;
    }
    if here.present.contains(id) {
        Err(link_id::Refused::Live)?;
    }
    here.hold(id);
    Ok(())
}

fn revive(here: &mut Here, id: &str) -> anyhow::Result<()> {
    if here.present.contains(id) {
        Err(link_id::Refused::Live)?;
    }
    here.kept.revive(id);
    Ok(())
}

/// A new agent's ID, reserved here.
pub fn reserve() -> anyhow::Result<String> {
    AGENTS.reserve()
}

/// Takes an arriving agent's ID ([`Book::admit`]).
pub fn admit(id: &str) -> anyhow::Result<()> {
    AGENTS.admit(id)
}

/// Takes `id` for agent `name` arriving by a move ([`Book::arrive`]).
pub fn arrive(name: &str, id: &str) -> anyhow::Result<()> {
    AGENTS.arrive(name, id)
}

/// Takes a restored agent's ID back ([`Book::restore`]).
pub fn restore(id: &str) -> anyhow::Result<()> {
    AGENTS.restore(id)
}

/// Retires a deleted agent's ID.
pub fn retire(id: &str) -> anyhow::Result<()> {
    AGENTS.retire(id)
}

/// Whether agent ID `id` was retired here.
pub fn is_retired(id: &str) -> anyhow::Result<bool> {
    AGENTS.is_retired(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ledger in a folder of its own, with `present` the agents here.
    fn ledger(
        root: &std::path::Path,
        present: &[&str],
        change: impl FnOnce(&mut Here) -> anyhow::Result<()>,
    ) -> anyhow::Result<link_id::Table> {
        let path = root.join("ids.toml");
        let present: BTreeSet<String> = present.iter().map(|id| id.to_string()).collect();
        with_at(&path, || Ok(present), change)?;
        with_at(&path, || Ok(BTreeSet::new()), |here| Ok(here.kept.clone()))
    }

    #[test]
    fn a_reserved_id_is_held_and_never_one_present() {
        let root = link_test_kit::TempDir::new("agent-ids-reserve");
        let mut made = String::new();
        let kept = ledger(&root, &["a1b2c3d4e5f6"], |here| {
            made = link_id::reserve(here)?;
            Ok(())
        })
        .unwrap();
        assert!(link_id::is_candidate(&made) && made != "a1b2c3d4e5f6");
        assert!(kept.live.contains(&made));
    }

    #[test]
    fn an_arrival_is_refused_a_retired_or_present_id() {
        let root = link_test_kit::TempDir::new("agent-ids-arrive");
        ledger(&root, &[], |here| {
            here.hold("a1b2c3d4e5f6");
            here.retire("a1b2c3d4e5f6");
            Ok(())
        })
        .unwrap();
        let retired = ledger(&root, &[], |here| take(here, "a1b2c3d4e5f6")).unwrap_err();
        assert!(retired.to_string().contains("since deleted"), "{retired}");
        let other = ledger(&root, &["b1b2c3d4e5f6"], |here| take(here, "b1b2c3d4e5f6"));
        assert!(other.unwrap_err().to_string().contains("exists"));
        // An agent that moved away and comes back: held, not present.
        let back = ledger(&root, &[], |here| {
            here.hold("c1b2c3d4e5f6");
            take(here, "c1b2c3d4e5f6")
        });
        assert!(back.unwrap().live.contains("c1b2c3d4e5f6"));
    }

    #[test]
    fn a_restore_takes_its_own_id_back() {
        let root = link_test_kit::TempDir::new("agent-ids-restore");
        let kept = ledger(&root, &[], |here| {
            here.hold("a1b2c3d4e5f6");
            here.retire("a1b2c3d4e5f6");
            revive(here, "a1b2c3d4e5f6")
        })
        .unwrap();
        assert!(kept.live.contains("a1b2c3d4e5f6") && kept.retired.is_empty());
        let taken = ledger(&root, &["a1b2c3d4e5f6"], |here| {
            revive(here, "a1b2c3d4e5f6")
        });
        assert!(taken.is_err());
    }

    #[test]
    fn an_id_is_written_with_its_kind() {
        assert_eq!(written("a1b2c3d4e5f6"), "agent:a1b2c3d4e5f6");
        assert_eq!(parse("agent:a1b2c3d4e5f6").unwrap(), "a1b2c3d4e5f6");
        for not in ["a1b2c3d4e5f6", "box:a1b2c3d4e5f6", "agent:", "agent:a b"] {
            assert!(parse(not).is_err(), "{not}");
        }
        assert_eq!(SERVICES.written("a1b2c3d4e5f6"), "service:a1b2c3d4e5f6");
        assert_eq!(
            SERVICES.parse("service:a1b2c3d4e5f6").unwrap(),
            "a1b2c3d4e5f6"
        );
        assert!(SERVICES.parse("agent:a1b2c3d4e5f6").is_err());
    }
}
