//! Who the agent is, both ways ([`link_harness_contract::harness::identity`]):
//! the spec's persona, owner and standing instructions in a block of
//! Hermes's `SOUL.md`, its one file of who the agent is, and the agent's
//! skills one of Hermes's external skills folders. Out of it, in ASP's
//! portable forms: what `SOUL.md` holds besides, the skills Hermes
//! made, neither bundled with it nor installed from its hub, and its
//! memory files.

use std::path::Path;

use link_format::WithinExt;
use link_harness_contract::asp;
use link_harness_contract::harness::identity::{self, Identity};
use serde_norway::Value;

/// The first words of the `SOUL.md` Hermes writes where there's none:
/// its own, never the owner's persona.
const SEEDED: &str = "You are Hermes Agent, built by Nous Research.";

/// Hermes's own separator between the entries of a memory file.
const ENTRY: &str = "\n§\n";

/// How an entry from elsewhere opens: `(from link) ...`. Such an entry
/// is never exported back.
const FROM: &str = "(from ";

/// The most of `MEMORY.md` Link fills, in characters: below Hermes's
/// limit (2,200), so its own memory tool can still add.
const FILL_MAX: usize = 1_600;

/// The entries of memory file `text` that are Hermes's own.
pub fn own_entries(text: &str) -> Vec<String> {
    text.split(ENTRY)
        .map(str::trim)
        .filter(|e| !e.is_empty() && !e.starts_with(FROM))
        .map(String::from)
        .collect()
}

/// Writes into Hermes's `MEMORY.md` in `home` what the agent remembers
/// elsewhere (`notes`, each not Hermes's own), an entry per fact, each
/// opening `(from <source>)`: after its own entries, those before them
/// taken out, as many as fit in [`FILL_MAX`]. The sources of the entries
/// it hadn't, each once.
pub fn remember(home: &Path, notes: &[asp::forms::Note]) -> anyhow::Result<Vec<String>> {
    identity::memory::reconcile(
        home,
        &home.join(".hermes/memories"),
        "hermes",
        notes,
        |text| own_entries(text).join(ENTRY),
    )?;
    let path = home.join(".hermes/memories/MEMORY.md");
    let old = identity::read(&path)?.unwrap_or_default();
    let before: Vec<&str> = old.split(ENTRY).map(str::trim).collect();
    let mut new_from: Vec<String> = Vec::new();
    let mut entries = own_entries(&old);
    let mut used = entries.join(ENTRY).chars().count();
    for note in notes {
        let source = note.source.as_deref().unwrap_or("link");
        if identity::memory::belongs(note, "hermes") || note.deleted {
            continue;
        }
        for fact in facts(&note.text) {
            let entry = format!("{FROM}{source}) {fact}");
            let more = entry.chars().count() + ENTRY.len();
            if entries.contains(&entry) || used + more > FILL_MAX {
                continue;
            }
            used += more;
            if !before.contains(&entry.as_str()) && !new_from.iter().any(|s| s == source) {
                new_from.push(source.to_string());
            }
            entries.push(entry);
        }
    }
    let new = match entries.is_empty() {
        true => String::new(),
        false => entries.join(ENTRY) + "\n",
    };
    if new.trim() == old.trim() {
        return Ok(new_from);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    link_files::write_private(&path, new.as_bytes())?;
    Ok(new_from)
}

/// The facts of a memory file's `text`: Hermes's entries, a list's
/// items, or its other lines, headings left out.
fn facts(text: &str) -> Vec<String> {
    text.split(ENTRY)
        .flat_map(str::lines)
        .map(|line| line.trim().trim_start_matches(['-', '*']).trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with(FROM))
        .map(String::from)
        .collect()
}

/// Whether `line` is the persona Hermes wrote.
fn seeded(line: &str) -> bool {
    line.starts_with(SEEDED)
}

/// `text` without the persona Hermes wrote, a line of its own.
fn unseeded(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|line| !seeded(line)).collect();
    lines.join("\n") + "\n"
}

/// Writes `settings.identity` into Hermes's files in `home`: nothing
/// from a Link before it.
pub fn configure(
    home: &Path,
    settings: &link_harness_contract::harness::Settings,
) -> anyhow::Result<()> {
    let Some(who) = &settings.identity else {
        return Ok(());
    };
    let soul = home.join(".hermes/SOUL.md");
    // A persona of the spec's takes the place of the one Hermes wrote.
    if who.persona.is_some()
        && let Some(text) = identity::read(&soul)?
        && text.lines().any(seeded)
    {
        link_files::write_private(&soul, unseeded(&text).as_bytes())?;
    }
    identity::write_block(&soul, who.all().as_deref(), who.persona.as_deref())?;
    let skills = home.join(identity::skills_relative());
    let path = home.join(".hermes/config.yaml");
    let mut config: Value = match std::fs::read(&path) {
        Ok(bytes) => serde_norway::from_slice(&bytes).within("reading Hermes's configuration")?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Value::Mapping(Default::default())
        }
        Err(err) => return Err(err.into()),
    };
    if config["skills"].is_null() {
        config["skills"] = Value::Mapping(Default::default());
    }
    if config["skills"]["external_dirs"].is_null() {
        config["skills"]["external_dirs"] = Value::Sequence(Vec::new());
    }
    let dirs = config["skills"]["external_dirs"]
        .as_sequence_mut()
        .within("reading Hermes's skills.external_dirs as a list")?;
    let dir = Value::String(skills.display().to_string());
    if dirs.contains(&dir) {
        return Ok(());
    }
    dirs.push(dir);
    link_files::write_private(&path, serde_norway::to_string(&config)?.as_bytes())?;
    Ok(())
}

/// What Hermes in `home` holds of its own, in ASP's portable forms:
/// `SOUL.md` outside Link's block as the persona of one `AGENTS.md`,
/// unless it's the one Hermes wrote; its skills but those it came with
/// or installed from its hub; and its memory files as notes.
pub fn export(home: &Path) -> anyhow::Result<asp::State> {
    let persona = identity::read(&home.join(".hermes/SOUL.md"))?
        .as_deref()
        .and_then(identity::outside_block)
        .map(|text| unseeded(&text).trim().to_string())
        .filter(|text| !text.is_empty());
    let root = home.join(".hermes/skills");
    let mut theirs: Vec<String> = std::fs::read_to_string(root.join(".bundled_manifest"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| Some(line.split(':').next()?.trim().to_string()))
        .filter(|name| !name.is_empty())
        .collect();
    if let Ok(lock) = std::fs::read(root.join(".hub/lock.json"))
        && let Ok(lock) = serde_json::from_slice::<serde_json::Value>(&lock)
        && let Some(installed) = lock["installed"].as_object()
    {
        theirs.extend(installed.keys().cloned());
    }
    let skills = identity::skill_folders(home, &root, &theirs)
        .into_iter()
        .filter_map(|folder| identity::skill_form(&home.join(folder)))
        .collect();
    let mut memory = Vec::new();
    for file in ["MEMORY.md", "USER.md"] {
        let note = identity::note(&home.join(".hermes/memories").join(file), file, "hermes")?;
        // Hermes's own entries: those from elsewhere are never exported back.
        memory.extend(note.map(|mut note| {
            note.text = own_entries(&note.text).join(ENTRY);
            note
        }));
    }
    Ok(asp::State {
        instructions: Some(
            Identity {
                persona,
                ..Identity::default()
            }
            .all()
            .unwrap_or_default(),
        ),
        skills: Some(skills),
        memory: Some(identity::memory::exported(home, "hermes", memory)?),
        ..asp::State::default()
    })
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

    /// Q-harnesses-18/F1: the spec's standing instructions and owner
    /// reach Hermes's model, in `SOUL.md`, which it reads at every turn.
    #[test]
    fn standing_instructions_and_owner_reach_soul_md() {
        let home = link_test_kit::TempDir::new("hermes-standing-owner");
        let who = Identity {
            owner: Some("The owner is called qa-owner.".into()),
            standing: Some("Answer briefly. qa-standing".into()),
            ..Identity::default()
        };
        configure(&home, &settings(Some(who))).unwrap();
        let soul = std::fs::read_to_string(home.join(".hermes/SOUL.md")).unwrap();
        assert!(
            soul.contains("## Your owner\n\nThe owner is called qa-owner."),
            "{soul}"
        );
        assert!(
            soul.contains("## Standing instructions\n\nAnswer briefly. qa-standing"),
            "{soul}"
        );
    }

    #[test]
    fn the_spec_s_identity_goes_into_soul_md_and_its_skills_folder_into_the_config() {
        let home = link_test_kit::TempDir::new("hermes-identity");
        std::fs::create_dir_all(home.join(".hermes")).unwrap();
        std::fs::write(home.join(".hermes/SOUL.md"), "Their own voice.\n").unwrap();
        std::fs::write(home.join(".hermes/config.yaml"), "model:\n  default: qa\n").unwrap();
        let who = Identity {
            persona: Some("qa-persona".into()),
            owner: Some("qa-owner".into()),
            standing: Some("qa-standing".into()),
            skills: vec!["qa".into()],
        };
        std::fs::write(
            home.join(".hermes/SOUL.md"),
            format!("{SEEDED} Be direct.\nTheir own voice.\n"),
        )
        .unwrap();
        configure(&home, &settings(Some(who))).unwrap();
        let soul = std::fs::read_to_string(home.join(".hermes/SOUL.md")).unwrap();
        assert!(
            !soul.contains(SEEDED),
            "the spec's persona takes the place of Hermes's: {soul}"
        );
        configure(&home, &settings(Some(Identity::default()))).unwrap();
        let soul = std::fs::read_to_string(home.join(".hermes/SOUL.md")).unwrap();
        assert_eq!(
            soul, "Their own voice.\n",
            "an identity emptied takes the block out"
        );
        let who = Identity {
            standing: Some("qa-standing".into()),
            ..Identity::default()
        };
        configure(&home, &settings(Some(who))).unwrap();
        let soul = std::fs::read_to_string(home.join(".hermes/SOUL.md")).unwrap();
        assert!(soul.contains("qa-standing") && soul.ends_with("Their own voice.\n"));
        let config = std::fs::read_to_string(home.join(".hermes/config.yaml")).unwrap();
        let dir = home.join(identity::skills_relative()).display().to_string();
        assert_eq!(config.matches(&dir).count(), 1, "{config}");
        assert!(config.contains("default: qa"));
    }

    #[test]
    fn what_it_remembers_elsewhere_goes_into_memory_md_after_its_own() {
        let home = link_test_kit::TempDir::new("hermes-remember");
        std::fs::create_dir_all(home.join(".hermes/memories")).unwrap();
        let path = home.join(".hermes/memories/MEMORY.md");
        std::fs::write(&path, "Its own.\n§\n(from link) stale\n").unwrap();
        let note = |source: &str, text: &str| asp::forms::Note {
            path: "x".into(),
            text: text.into(),
            source: Some(source.into()),
            deleted: false,
        };
        let notes = [
            note("link", "# Notes\n- qa-link-note\n"),
            note("openclaw", "- qa-openclaw-note\n- qa-link-note\n"),
            note("hermes", "Its own."),
            note("link", &"x".repeat(FILL_MAX)),
        ];
        // Each source of an entry it hadn't, once: Link's stale one went.
        assert_eq!(remember(&home, &notes).unwrap(), ["link", "openclaw"]);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "Its own.\n§\n(from link) qa-link-note\n§\n(from openclaw) qa-openclaw-note\n§\n(from openclaw) qa-link-note\n"
        );
        assert_eq!(own_entries(&text), ["Its own."], "never exported back");
        assert!(remember(&home, &notes).unwrap().is_empty(), "none new");
        remember(&home, &[]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Its own.\n");
    }

    #[test]
    fn a_link_before_identity_leaves_hermes_s_files_alone() {
        let home = link_test_kit::TempDir::new("hermes-identity-old");
        configure(&home, &settings(None)).unwrap();
        assert!(!home.join(".hermes").exists());
    }

    #[test]
    fn its_own_persona_skills_and_memory_come_out_but_not_what_it_came_with() {
        let home = link_test_kit::TempDir::new("hermes-native");
        let skills = home.join(".hermes/skills");
        for skill in ["notes/summarize", "bundled", "from-hub", "qa"] {
            let name = skill.rsplit('/').next().unwrap();
            std::fs::create_dir_all(skills.join(skill)).unwrap();
            let text = format!("---\nname: {name}\ndescription: {name} skill\n---\n");
            std::fs::write(skills.join(skill).join("SKILL.md"), text).unwrap();
        }
        std::fs::write(skills.join(".bundled_manifest"), "bundled:abc\n").unwrap();
        std::fs::create_dir_all(skills.join(".hub")).unwrap();
        std::fs::write(
            skills.join(".hub/lock.json"),
            r#"{"installed": {"from-hub": {}}}"#,
        )
        .unwrap();
        assert_eq!(export(&home).unwrap().instructions.as_deref(), Some(""));
        let soul = identity::with_block("Call me Ada.", Some("from the spec"));
        std::fs::write(home.join(".hermes/SOUL.md"), soul).unwrap();
        std::fs::create_dir_all(home.join(".hermes/memories")).unwrap();
        std::fs::write(home.join(".hermes/memories/MEMORY.md"), "- qa remembers\n").unwrap();
        let state = export(&home).unwrap();
        let who = Identity::of_agents_md(state.instructions.as_deref().unwrap());
        assert_eq!(who.persona.as_deref(), Some("Call me Ada."));
        let names: Vec<_> = state.skills.unwrap().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["summarize", "qa"]);
        let memory = state.memory.unwrap();
        assert_eq!(memory.len(), 1);
        assert_eq!(memory[0].source.as_deref(), Some("hermes:MEMORY.md"));
        std::fs::write(home.join(".hermes/SOUL.md"), format!("{SEEDED} Be direct.")).unwrap();
        assert_eq!(export(&home).unwrap().instructions.as_deref(), Some(""));
        let soul = format!(
            "{}{SEEDED} Be direct.\nCall me Ada.\n",
            identity::with_block("", Some("x"))
        );
        std::fs::write(home.join(".hermes/SOUL.md"), soul).unwrap();
        let state = export(&home).unwrap();
        let who = Identity::of_agents_md(state.instructions.as_deref().unwrap());
        assert_eq!(
            who.persona.as_deref(),
            Some("Call me Ada."),
            "its own, after the one Hermes wrote"
        );
    }
}
