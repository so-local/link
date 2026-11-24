//! Each kind's owner, asked through its module's answers to the archive
//! questions ([`link_archive_contract`]): the module `layout.toml`'s
//! `[owners]` names for the kind, when it's installed and answers them.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;

use anyhow::Context;
use link_archive::{Adopted, Missing, Owner, Owners, Replaced, Running, ThingFolders};
use link_archive_contract as contract;
use link_core::answers::Answerer;
use link_files::layout::map::Entry;

/// A kind's owner, by its module's answers.
pub struct Asked {
    module: String,
    answerers: BTreeMap<&'static str, Answerer>,
}

impl Asked {
    fn ask<I: serde::Serialize, O: serde::de::DeserializeOwned>(
        &self,
        question: &link_core::answers::Question<I, O>,
        input: &I,
    ) -> anyhow::Result<O> {
        let answerer = self.answerers.get(question.name).with_context(|| {
            format!(
                "the {} module doesn't answer {}",
                self.module, question.name
            )
        })?;
        answerer.reply(question, input, Stdio::piped())
    }
}

/// A thing as the questions name it: its folders by their locations'
/// keys.
fn thing(folders: &ThingFolders) -> contract::Thing {
    contract::Thing {
        kind: folders.kind.clone(),
        name: folders.name.clone(),
        folders: folders
            .folders
            .iter()
            .map(|(home, path)| (Entry::Home(*home).name(), path.clone()))
            .collect(),
    }
}

impl Owner for Asked {
    fn check(&self, folders: &ThingFolders) -> anyhow::Result<()> {
        self.ask(&contract::CHECK, &thing(folders)).map(|_| ())
    }

    fn running(&self, folders: &ThingFolders) -> anyhow::Result<Option<Running>> {
        let running = self.ask(&contract::RUNNING, &thing(folders))?;
        Ok(running.stop.map(|stop| Running { stop }))
    }

    fn carry(&self, folders: &ThingFolders, staging: &Path) -> anyhow::Result<Vec<Replaced>> {
        let carried = self.ask(
            &contract::CARRY,
            &contract::Carry {
                thing: thing(folders),
                staging: staging.into(),
            },
        )?;
        Ok(carried
            .replaced
            .into_iter()
            .map(|r| Replaced {
                file: r.file,
                with: r.with,
            })
            .collect())
    }

    fn missing(&self, folders: &ThingFolders) -> anyhow::Result<Vec<Missing>> {
        let missing = self.ask(&contract::MISSING, &thing(folders))?;
        Ok(missing
            .needs
            .into_iter()
            .map(|need| Missing {
                what: need.what,
                setup: need.setup,
            })
            .collect())
    }

    fn adopt(&self, folders: &ThingFolders, was: &str) -> anyhow::Result<Adopted> {
        let adopted = self.ask(
            &contract::ADOPT,
            &contract::Adopt {
                thing: thing(folders),
                was: was.into(),
            },
        )?;
        Ok(Adopted {
            notes: adopted.notes,
            next: adopted.next,
        })
    }
}

/// The owners of the kinds an archive carries here: for each kind in
/// `layout.toml`, its owner's module, when it's installed and answers all
/// five questions.
pub struct Installed {
    owners: BTreeMap<String, Asked>,
}

impl Installed {
    pub fn here() -> Self {
        let listed = link_core::modules::list();
        let mut owners = BTreeMap::new();
        for owner in link_files::layout::owners().values() {
            let Some(kind) = &owner.kind else { continue };
            let answerers: BTreeMap<&'static str, Answerer> = [
                (contract::CHECK.name, contract::CHECK.answerers_in(&listed)),
                (
                    contract::RUNNING.name,
                    contract::RUNNING.answerers_in(&listed),
                ),
                (contract::CARRY.name, contract::CARRY.answerers_in(&listed)),
                (
                    contract::MISSING.name,
                    contract::MISSING.answerers_in(&listed),
                ),
                (contract::ADOPT.name, contract::ADOPT.answerers_in(&listed)),
            ]
            .into_iter()
            .filter_map(|(name, answerers)| {
                answerers
                    .into_iter()
                    .find(|a| a.module == owner.module)
                    .map(|a| (name, a))
            })
            .collect();
            if answerers.len() == contract::QUESTIONS.len() {
                owners.insert(
                    kind.clone(),
                    Asked {
                        module: owner.module.clone(),
                        answerers,
                    },
                );
            }
        }
        Self { owners }
    }
}

impl Owners for Installed {
    fn of(&self, kind: &str) -> Option<&dyn Owner> {
        self.owners.get(kind).map(|o| o as &dyn Owner)
    }
}
