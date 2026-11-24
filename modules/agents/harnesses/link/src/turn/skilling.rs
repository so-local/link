//! The agent's skills, run as commands ([`crate::commands::skills`]):
//! the owner's from its `skills/`, through the store, and the
//! repository's from its folders above Home.

use super::{Harness, off_runtime};
use crate::agent::Agent;
use crate::commands::{self, Command, skills};
use crate::event::Event;
use link_files::reading::ContentDigest;
use link_harness_wire::evidence::{
    Evidence, ReadAuthority, Record, SkillAnchor, SkillLoad, SkillRoute, SkillSource,
};

pub(super) struct LoadedSkill {
    pub text: String,
    pub evidence: Evidence,
}

impl Harness {
    /// `agent`'s skills, the owner's first, then the repository's.
    pub(crate) fn found_skills(&self, agent: &Agent) -> anyhow::Result<Vec<skills::Found>> {
        let own = off_runtime(|| self.store.skills(agent))?;
        let repository = match self.workspace(agent) {
            Some(home) => {
                off_runtime(|| crate::repository::skills(&home, crate::store::SKILLS_MAX))
            }
            None => Vec::new(),
        };
        Ok(skills::found(&own, &repository))
    }

    /// The message the skill `message` runs asks its turn with, of
    /// `commands`, the ones that are on: none where it runs none, or its
    /// spec's `[commands] skills` is off.
    pub(super) fn skill_asked(
        &self,
        agent: &Agent,
        message: &Event,
        on: bool,
        commands: &[Command],
    ) -> anyhow::Result<Option<LoadedSkill>> {
        if !on || commands::owners_slash(message).is_none() {
            return Ok(None);
        }
        let found = self.found_skills(agent)?;
        let Some((skill, arguments)) = skills::invoked(message, &found, commands) else {
            return Ok(None);
        };
        let (read, source) = match &skill.whose {
            skills::Whose::Owner => {
                let path = format!(
                    "{}/{}/{}",
                    link_harness_contract::harness::layout::SKILLS,
                    skill.name,
                    link_harness_wire::skill::FILE
                );
                (
                    off_runtime(|| self.store.instruction_read(agent, &path))?,
                    SkillSource::Owned { path },
                )
            }
            skills::Whose::Repository(path) => {
                let home = self.workspace(agent);
                let root = home
                    .as_ref()
                    .and_then(|home| crate::repository::folders(home).last().cloned());
                let source = root
                    .as_ref()
                    .and_then(|root| path.strip_prefix(root).ok())
                    .unwrap_or(path)
                    .display()
                    .to_string();
                (
                    crate::repository::read_file_receipt(path, &source),
                    SkillSource::Repository { path: source },
                )
            }
        };
        Ok(read.map(|read| {
            let text = skills::asked(&skill.name, &read.text, &arguments);
            let complete = read.receipt.covers_source();
            let load = SkillLoad {
                anchor: SkillAnchor::Owner,
                source,
                route: SkillRoute::Invocation,
                authority: ReadAuthority::Runtime,
                supplied: Some(ContentDigest::of(
                    skills::instruction_body(&read.text).as_bytes(),
                )),
                model_view: ContentDigest::of(text.as_bytes()),
                complete,
                omissions: if complete {
                    vec![]
                } else {
                    vec!["instruction source was partial or decoded with loss".into()]
                },
                original_input: Some(message.text.clone()),
                read: read.receipt,
            };
            LoadedSkill {
                text,
                evidence: Evidence::new(Record::SkillLoaded {
                    load: Box::new(load),
                }),
            }
        }))
    }
}
