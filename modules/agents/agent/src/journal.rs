//! A migration's journal, `~/.lnk/safe/agents/journal/<agent>.toml`: the whole plan,
//! written before anything is done, then each step marked as it ends.
//! Running the migration again resumes at the first step not done;
//! `lnk agent delete` clears a partial one, undoing each step done, last
//! first. Nothing is touched that the journal doesn't name. Every path
//! in it is relative to its folder: the home folder for the original
//! install, the agent's harness home for what it was copied to.

use link_format::WithinExt;
use std::path::PathBuf;

use link_files::Relative;
use serde::{Deserialize, Serialize};

/// One step of a migration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
// Flattened into its entry, so the entry, not each step, sets which
// fields there are.
#[serde(tag = "step", rename_all = "kebab-case")]
pub enum Step {
    /// The agent made under Link, with the harness it had.
    Agent,
    /// Import schedules from the managed copy, retaining every omission.
    Schedules {
        at: Relative,
        names: Vec<String>,
        not_carried: Vec<String>,
    },
    /// A file or folder of the install copied into the agent's harness
    /// home, and checked there by its hash.
    Copy {
        /// In the home folder.
        from: Relative,
        /// In the agent's harness home.
        to: Relative,
        /// In the home folder, inside `from`: left out, other agents'.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        leaving: Vec<Relative>,
    },
    /// A secret of the install's, by its variable: taken out of the
    /// copy, left in the original for its owner to give sealed.
    Secret { name: String },
    /// A hosted model's key of the install's, by its variable: the
    /// agent's model key, kept as `lnk agent start` keeps one.
    Model { name: String },
    /// A service's key of the install's, by its variable: given to the
    /// agent sealed as `variables.toml` says.
    Sealed { name: String },
    /// The install's own service, stopped and kept from starting: never
    /// deleted, so clearing starts it again.
    Service {
        /// In the home folder.
        at: Relative,
    },
}

/// A step of the plan, and whether it's done.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(flatten)]
    pub step: Step,
    #[serde(default)]
    pub done: bool,
}

link_files::specification!(Journal, include_str!("journal.schema.toml"));

/// Each agent's migration under way, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Journal>()
}

/// A migration's plan and how far it went.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    /// The agent it makes.
    pub agent: String,
    /// The harness it keeps.
    pub harness: String,
    #[serde(default, rename = "steps")]
    pub entries: Vec<Entry>,
}

impl Journal {
    /// A new plan, nothing done.
    pub fn plan(agent: &str, harness: &str, steps: Vec<Step>) -> Self {
        Self {
            agent: agent.to_string(),
            harness: harness.to_string(),
            entries: steps
                .into_iter()
                .map(|step| Entry { step, done: false })
                .collect(),
        }
    }

    /// `~/.lnk/safe/agents/journal`.
    pub fn dir() -> anyhow::Result<PathBuf> {
        link_files::layout::Hidden::MigrationJournal.try_path()
    }

    /// Where agent `agent`'s journal is.
    pub fn path_of(agent: &str) -> anyhow::Result<PathBuf> {
        link_machine::machine::check_name(agent)?;
        Ok(Self::dir()?.join(format!("{agent}.toml")))
    }

    /// Agent `agent`'s journal: a migration begun and not finished.
    pub fn load(agent: &str) -> anyhow::Result<Option<Self>> {
        let path = Self::path_of(agent)?;
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let journal: Self = link_files::saved::parse(&text)
                    .within(format!("reading {}", path.display()))?;
                anyhow::ensure!(
                    journal.agent == agent,
                    "{} is {}'s journal, not {agent}'s",
                    path.display(),
                    journal.agent
                );
                Ok(Some(journal))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).within(format!("reading {}", path.display())),
        }
    }

    /// A migration of harness `harness` begun and not finished, whichever
    /// agent it makes.
    pub fn of_harness(harness: &str) -> anyhow::Result<Option<Self>> {
        let Ok(entries) = std::fs::read_dir(Self::dir()?) else {
            return Ok(None);
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(agent) = name.strip_suffix(".toml")
                && let Ok(Some(journal)) = Self::load(agent)
                && journal.harness == harness
            {
                return Ok(Some(journal));
            }
        }
        Ok(None)
    }

    /// Writes it whole, at once.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path_of(&self.agent)?;
        link_files::saved::write(&path, self).within(format!("writing {}", path.display()))
    }

    /// Whether every step is done.
    pub fn finished(&self) -> bool {
        self.entries.iter().all(|e| e.done)
    }

    /// Runs each step not done yet, in order, with `run`, marking and
    /// saving it as it ends; the first that fails stops it there, and
    /// running it again resumes at that step. The journal goes once every
    /// step is done.
    pub fn run(&mut self, mut run: impl FnMut(&Step) -> anyhow::Result<()>) -> anyhow::Result<()> {
        self.save()?;
        for i in 0..self.entries.len() {
            if self.entries[i].done {
                continue;
            }
            run(&self.entries[i].step)?;
            self.entries[i].done = true;
            self.save()?;
        }
        self.forget()
    }

    /// Undoes each step done, last first, with `undo`, saving after each,
    /// then forgets the journal: a partial migration cleared. A step that
    /// can't be undone stops it there, kept for the next try.
    pub fn clear(
        &mut self,
        mut undo: impl FnMut(&Step) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        for i in (0..self.entries.len()).rev() {
            if !self.entries[i].done {
                continue;
            }
            undo(&self.entries[i].step)?;
            self.entries[i].done = false;
            self.save()?;
        }
        self.forget()
    }

    /// Deletes the journal.
    fn forget(&self) -> anyhow::Result<()> {
        let path = Self::path_of(&self.agent)?;
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(e).within(format!("removing {}", path.display()))
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
