//! Who the agent is, both ways ([`link_harness_contract::harness::identity`]):
//! the spec's persona, owner and standing instructions in a block of
//! `dsh`'s user-level `AGENTS.md`, which it reads at each session's start,
//! and the agent's skills one of the skill folders it scans
//! ([`skills_row`]). Out of it, what that `AGENTS.md` holds besides and the
//! skills in its own folder, in ASP's portable forms. It keeps no memory of
//! its own: Link's memory reaches it as a tool.

use std::path::{Path, PathBuf};

use link_harness_contract::asp;
use link_harness_contract::asp::methods::NotTaken;
use link_harness_contract::harness::identity;
use serde_json::{Value, json};

use crate::state::LAYOUT;

/// `dsh`'s own home in home `home`.
fn dsh_home(home: &Path) -> PathBuf {
    home.join(&LAYOUT.home)
}

/// Its user-level `AGENTS.md` in home `home`.
fn instructions(home: &Path) -> PathBuf {
    dsh_home(home).join(&LAYOUT.instructions)
}

/// The skills folder of its own in home `home`.
fn skills(home: &Path) -> PathBuf {
    dsh_home(home).join(&LAYOUT.skills)
}

/// Writes `settings.identity` into `dsh`'s `AGENTS.md` in `home`, the rest
/// of the file as its owner or another harness left it.
pub fn configure(
    home: &Path,
    settings: &link_harness_contract::harness::Settings,
) -> anyhow::Result<()> {
    let Some(who) = &settings.identity else {
        return Ok(());
    };
    identity::write_block(
        &instructions(home),
        who.all().as_deref(),
        who.persona.as_deref(),
    )
}

/// The row of Link's patch layer that adds the skills Link copies into
/// `home` to the folders `dsh` scans, after its project's and before its
/// own. The plugin's other settings keep their defaults.
pub fn skills_row(home: &Path) -> Value {
    json!({"id": "skill-filesystem", "config": {
        "customSkillDirs": [home.join(identity::skills_relative())],
    }})
}

/// What `dsh` in `home` holds of its own, in ASP's portable forms: its
/// `AGENTS.md` outside Link's block, and the skills in its own folder,
/// but those under a folder starting with `.` (its `.system` skills).
pub fn export(home: &Path) -> anyhow::Result<asp::State> {
    let text = identity::read(&instructions(home))?
        .as_deref()
        .and_then(identity::outside_block)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    let skills = identity::skill_folders(home, &skills(home), &[])
        .into_iter()
        .filter_map(|folder| identity::skill_form(&home.join(folder)))
        .collect();
    Ok(asp::State {
        instructions: Some(text.unwrap_or_default()),
        skills: Some(skills),
        ..asp::State::default()
    })
}

/// Takes `state`'s instructions and skills into `dsh` in `home`, and says
/// why each other kind isn't taken here.
pub fn import(
    home: &Path,
    state: &asp::State,
    imported: &mut asp::methods::Imported,
) -> anyhow::Result<()> {
    if let Some(text) = &state.instructions {
        identity::write_outside(&instructions(home), text)?;
        imported.taken.push(asp::Kind::Instructions);
    }
    if let Some(given) = &state.skills {
        for skill in given {
            identity::write_skill(&skills(home), skill)?;
        }
        imported.taken.push(asp::Kind::Skills);
    }
    let mut not = |kind, reason: &str| {
        imported.not_taken.push(NotTaken {
            kind,
            reason: reason.into(),
        })
    };
    if state.memory.is_some() {
        not(
            asp::Kind::Memory,
            "DeepSeek Harness keeps no memory of its own; Link's memory reaches it as a tool",
        );
    }
    if state.tools.is_some() {
        not(
            asp::Kind::Tools,
            "DeepSeek Harness takes its MCP servers from Link with each session",
        );
    }
    if state.settings.as_ref().is_some_and(|s| !s.is_empty()) {
        not(
            asp::Kind::Settings,
            "DeepSeek Harness takes its settings from Link's",
        );
    }
    if state.schedules.is_some() {
        not(
            asp::Kind::Schedules,
            "Link owns portable schedules; DeepSeek Harness's ACP profile runs none",
        );
    }
    if state.sessions.is_some() {
        not(
            asp::Kind::Sessions,
            "DeepSeek owns its native format; the hub supplies portable continuation separately",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_harness_contract::harness::{Settings, identity::Identity};

    fn settings(identity: Option<Identity>) -> Settings {
        Settings {
            identity,
            ..Settings::default()
        }
    }

    /// The spec's persona, owner and standing instructions reach `dsh`'s
    /// `AGENTS.md` in Link's block, and what its owner wrote there stays.
    #[test]
    fn the_spec_reaches_agents_md_beside_its_owners_text() {
        let home = link_test_kit::TempDir::new("deepseek-identity");
        let file = instructions(&home);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "Prefer short commits.\n").unwrap();
        let who = Identity {
            persona: Some("You are qa-persona.".into()),
            owner: Some("The owner is qa-owner.".into()),
            standing: Some("Answer briefly.".into()),
            ..Identity::default()
        };
        configure(&home, &settings(Some(who))).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        for part in [
            "qa-persona",
            "qa-owner",
            "Answer briefly.",
            "Prefer short commits.",
        ] {
            assert!(text.contains(part), "{part}: {text}");
        }
        // Nothing handed, nothing written.
        let other = link_test_kit::TempDir::new("deepseek-identity-none");
        configure(&other, &settings(None)).unwrap();
        assert!(!instructions(&other).exists());
    }

    /// Out of it, its own text and skills, never Link's block or its own
    /// `.system` skills; into another, the same, which it exports again.
    #[test]
    fn its_own_instructions_and_skills_go_both_ways() {
        let home = link_test_kit::TempDir::new("deepseek-identity-export");
        configure(
            &home,
            &settings(Some(Identity {
                persona: Some("You are qa-persona.".into()),
                ..Identity::default()
            })),
        )
        .unwrap();
        let file = instructions(&home);
        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, format!("{text}\nKeep notes in docs/.\n")).unwrap();
        for (folder, name) in [("triage", "triage"), (".system/bundled", "bundled")] {
            let dir = skills(&home).join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: Sort the inbox.\n---\nSort it.\n"),
            )
            .unwrap();
        }
        let state = export(&home).unwrap();
        assert_eq!(state.instructions.as_deref(), Some("Keep notes in docs/."));
        let names: Vec<_> = state
            .skills
            .iter()
            .flatten()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(names, ["triage"]);

        let other = link_test_kit::TempDir::new("deepseek-identity-import");
        let mut imported = asp::methods::Imported::default();
        let incoming = asp::State {
            memory: Some(Vec::new()),
            ..state.clone()
        };
        import(&other, &incoming, &mut imported).unwrap();
        assert_eq!(imported.taken, [asp::Kind::Instructions, asp::Kind::Skills]);
        assert_eq!(imported.not_taken.len(), 1);
        assert_eq!(imported.not_taken[0].kind, asp::Kind::Memory);
        let again = export(&other).unwrap();
        assert_eq!(again.instructions, state.instructions);
        assert_eq!(
            again.skills.unwrap()[0].files,
            state.skills.unwrap()[0].files
        );
    }

    /// The skills Link copies are a folder `dsh` scans.
    #[test]
    fn links_skills_are_a_folder_it_scans() {
        let home = Path::new("/home/agent");
        let row = skills_row(home);
        assert_eq!(row["id"], "skill-filesystem");
        assert_eq!(
            row["config"]["customSkillDirs"][0],
            json!(home.join(identity::skills_relative()))
        );
    }
}
