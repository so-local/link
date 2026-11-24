//! The owner's skills as commands: `/skill:<name>` runs a skill, the
//! owner's own (`skills/`) before the repository's (`.agents/skills/`,
//! `.claude/skills/`). A bare `/<name>` runs the owner's skill even where
//! a command has that name, so the owner can replace any of Link's, and a
//! repository's only where no command has it, so a cloned repository
//! never takes over `/stop`. A skill is a prompt: it runs as a turn of
//! its own, asked with what its `SKILL.md` says past its front matter.

use std::path::PathBuf;

use super::{Command, owners_slash};
use crate::event::Event;

/// Whose a skill is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Whose {
    /// The owner's, in the agent's `skills/`.
    Owner,
    /// The repository's, its `SKILL.md` at this path.
    Repository(PathBuf),
}

/// A skill the agent has: its name, what it's for, and whose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub description: String,
    pub whose: Whose,
}

/// The agent's skills, the owner's `own` first, then the repository's.
pub fn found(
    own: &[link_harness_wire::skill::Skill],
    repository: &[crate::repository::Skill],
) -> Vec<Found> {
    own.iter()
        .map(|s| Found {
            name: s.name.clone(),
            description: s.description.clone(),
            whose: Whose::Owner,
        })
        .chain(repository.iter().map(|r| Found {
            name: r.skill.name.clone(),
            description: r.skill.description.clone(),
            whose: Whose::Repository(r.path.clone()),
        }))
        .collect()
}

/// The skill `message` runs, of `found`, and what follows its name: the
/// owner's own message `/skill:<name>`, or a bare `/<name>` as the
/// module says, `commands` the ones that are on. None for any other.
pub fn invoked<'a>(
    message: &Event,
    found: &'a [Found],
    commands: &[Command],
) -> Option<(&'a Found, String)> {
    let (name, arguments) = owners_slash(message)?;
    let named = |name: &str, whose: fn(&Whose) -> bool| {
        found.iter().find(|f| f.name == name && whose(&f.whose))
    };
    let any = |_: &Whose| true;
    let owners = |w: &Whose| *w == Whose::Owner;
    let skill = match name.strip_prefix("skill:") {
        // The skill namespace's own actions (`/skill:write`) stay theirs.
        Some(name) if !commands.iter().any(|c| c.name == format!("skill:{name}")) => {
            named(name, any)
        }
        Some(_) => None,
        None => named(name, owners).or_else(|| {
            let command = commands
                .iter()
                .any(|c| c.name == name || c.aliases.iter().any(|a| a == name));
            (!command).then(|| named(name, any)).flatten()
        }),
    }?;
    Some((skill, arguments.to_string()))
}

/// The message a skill named `name` runs as, `body` its `SKILL.md` and
/// `arguments` what the owner said after its name.
pub fn asked(name: &str, body: &str, arguments: &str) -> String {
    let body = instruction_body(body);
    match arguments.trim() {
        "" => format!("/skill:{name}\n\n{body}"),
        asked => format!("/skill:{name} {asked}\n\n{body}\n\n{asked}"),
    }
}

/// The exact skill body the invocation supplies, apart from its arguments.
pub fn instruction_body(body: &str) -> &str {
    body.splitn(3, "---\n").nth(2).unwrap_or(body).trim()
}

/// What `/skill:list` says of `found`: each skill, its name and what
/// it's for, whose, and which a nearer one of its name replaces.
pub fn listed(found: &[Found]) -> String {
    if found.is_empty() {
        return "No skills: yours go in skills/, a repository's in .agents/skills/ or .claude/skills/.".into();
    }
    let mut told = String::from("Skills:");
    for (i, skill) in found.iter().enumerate() {
        let whose = match &skill.whose {
            Whose::Owner => "yours".to_string(),
            Whose::Repository(path) => format!("the repository's, {}", path.display()),
        };
        told.push_str(&format!(
            "\n/skill:{} - {} ({whose})",
            skill.name, skill.description
        ));
        if found[..i].iter().any(|f| f.name == skill.name) {
            told.push_str(", replaced by yours");
        }
    }
    told
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{Does, list};

    fn skill(name: &str, whose: Whose) -> Found {
        Found {
            name: name.into(),
            description: format!("{name}'s"),
            whose,
        }
    }

    #[test]
    fn the_owners_skill_wins_and_a_repositorys_never_takes_over_a_command() {
        let repo = || Whose::Repository("/r/.agents/skills/x/SKILL.md".into());
        let found = [
            skill("stop", Whose::Owner),
            skill("deploy", repo()),
            skill("new", repo()),
            skill("stop", repo()),
        ];
        let commands = list(&crate::config::Config::default());
        let run = |text: &str| {
            invoked(&Event::user(text), &found, &commands).map(|(f, a)| (f.clone(), a))
        };
        // The owner's own replaces a command of its name.
        assert_eq!(run("/stop").unwrap().0, found[0]);
        // A repository's runs by its name only where no command has it.
        assert_eq!(
            run("/deploy now").unwrap(),
            (found[1].clone(), "now".into())
        );
        assert_eq!(run("/new"), None);
        // `/skill:<name>` always runs the skill, the owner's first.
        assert_eq!(run("/skill:new").unwrap().0, found[2]);
        assert_eq!(run("/skill:stop").unwrap().0, found[0]);
        assert_eq!(run("/skill:none"), None);
        // Anyone else's is text.
        let theirs = Event {
            from: Some("member:telegram:7".into()),
            ..Event::user("/stop")
        };
        assert_eq!(invoked(&theirs, &found, &commands), None);
        assert!(commands.iter().any(|c| c.does == Does::Stop));
    }

    #[test]
    fn a_skill_is_asked_with_its_instructions() {
        let body = "---\nname: deploy\ndescription: Deploys.\n---\nRun the deploy.\n";
        assert_eq!(
            asked("deploy", body, ""),
            "/skill:deploy\n\nRun the deploy."
        );
        assert_eq!(
            asked("deploy", body, "staging"),
            "/skill:deploy staging\n\nRun the deploy.\n\nstaging"
        );
        let listed = listed(&[
            skill("a", Whose::Owner),
            skill("a", Whose::Repository("/r/SKILL.md".into())),
        ]);
        assert!(listed.contains("/skill:a - a's (yours)"), "{listed}");
        assert!(listed.contains("(the repository's, /r/SKILL.md), replaced by yours"));
    }
}
