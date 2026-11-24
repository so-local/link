//! Who the agent is, both ways between Link's spec and a harness's own
//! files: its persona, its owner, its standing instructions and its
//! skills.
//!
//! Into a harness, Link hands them in [`Settings::identity`]
//! (`super::Settings`), and copies the agent's skills into the harness's
//! home at [`skills_relative`]; its adapter writes each text into a block of its
//! own files ([`write_block`]), the rest of each file as the harness or
//! its owner left it, and points the harness's skills at the copy.
//!
//! Out of one, the adapter answers `lnk-<name> state export` with what
//! the harness holds of its own in ASP's portable forms
//! ([`link_agent_state_protocol::State`]): its files' text outside Link's block
//! ([`outside_block`]) as one `AGENTS.md` ([`Identity::all`]), each
//! skill it made as an Agent Skills folder ([`skill_form`]), and its
//! memory as notes. Link takes each where the agent's spec has none
//! ([`Identity::of_agents_md`]), so a switch carries what the agent
//! became on the harness it leaves.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub mod memory;

/// Where in a harness's home Link copies the agent's skills, a folder
/// per skill (`<name>/SKILL.md`), replaced whole at each start.
pub fn skills_relative() -> std::path::PathBuf {
    link_files::layout::Hidden::AgentPlaces
        .relative_to_home()
        .join("skills")
}

/// The line opening Link's block in a harness's file.
pub const BEGIN: &str = "<!-- link:begin: written by Link from the agent's spec at each start; \
                         change the agent's own instructions/ instead -->";

/// The line closing it.
pub const END: &str = "<!-- link:end -->";

/// The most of each text read or handed, in bytes: the spec's sections
/// are cut far below it.
pub const TEXT_MAX: usize = 256 << 10;

/// Who the agent is, as its spec says: each text the spec turns on,
/// read from the agent's own folder, and its skills' names.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    /// Who it is (`instructions/persona.md`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// Who its owner is (`instructions/owner.md`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// What its owner tells it to be (`instructions/instruction.md`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standing: Option<String>,
    /// Its skills, by name, copied to [`skills_relative`] in the harness's home.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
}

impl Identity {
    /// The persona and owner as one Markdown text, each under its
    /// heading, for a harness that reads them from one file: none when
    /// neither is set.
    pub fn who(&self) -> Option<String> {
        sections(&[
            ("Who you are", self.persona.as_deref()),
            ("Your owner", self.owner.as_deref()),
        ])
    }

    /// The standing instructions under their heading: none when unset.
    pub fn instructions(&self) -> Option<String> {
        sections(&[("Standing instructions", self.standing.as_deref())])
    }

    /// Every text, each under its heading, for a harness that reads them
    /// all from one file.
    pub fn all(&self) -> Option<String> {
        sections(&[
            ("Who you are", self.persona.as_deref()),
            ("Your owner", self.owner.as_deref()),
            ("Standing instructions", self.standing.as_deref()),
        ])
    }
}

/// The headings [`Identity::all`] puts each text under.
const HEADINGS: [&str; 3] = ["Who you are", "Your owner", "Standing instructions"];

impl Identity {
    /// The texts of `AGENTS.md` `text`, as [`Identity::all`] writes
    /// them: each under its heading; what's under none of them is
    /// standing instructions, an `AGENTS.md` of a harness's own being
    /// what it was told to be.
    pub fn of_agents_md(text: &str) -> Self {
        let mut parts: [Vec<&str>; 3] = Default::default();
        let mut at = 2;
        for line in text.lines() {
            if let Some(i) = line
                .strip_prefix("## ")
                .and_then(|h| HEADINGS.iter().position(|known| *known == h.trim()))
            {
                at = i;
                continue;
            }
            parts[at].push(line);
        }
        let [persona, owner, standing] = parts.map(|lines| {
            let text = lines.join("\n");
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        });
        Self {
            persona,
            owner,
            standing,
            skills: Vec::new(),
        }
    }
}

fn sections(parts: &[(&str, Option<&str>)]) -> Option<String> {
    let parts: Vec<String> = parts
        .iter()
        .filter_map(|(heading, text)| {
            let text = text.map(str::trim).filter(|t| !t.is_empty())?;
            Some(format!("## {heading}\n\n{text}"))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// `text` with Link's block replaced by `body`, or added first when it
/// has none; with no `body`, the block taken out. The rest stays as it
/// was. A block left open (a `BEGIN` with no `END`) runs to the end.
pub fn with_block(text: &str, body: Option<&str>) -> String {
    let (before, after) = match text.find(BEGIN) {
        Some(start) => {
            let rest = &text[start + BEGIN.len()..];
            let after = rest.find(END).map_or("", |end| &rest[end + END.len()..]);
            (&text[..start], after)
        }
        None => ("", text),
    };
    let (before, after) = (before.trim_end(), after.trim());
    let block = body.map(|body| format!("{BEGIN}\n{}\n{END}", body.trim()));
    let parts: Vec<&str> = [Some(before), block.as_deref(), Some(after)]
        .into_iter()
        .flatten()
        .filter(|p| !p.is_empty())
        .collect();
    match parts.is_empty() {
        true => String::new(),
        false => parts.join("\n\n") + "\n",
    }
}

/// `text` without Link's block, trimmed: what the harness or its owner
/// wrote. None when that's nothing.
pub fn outside_block(text: &str) -> Option<String> {
    let outside = with_block(text, None);
    let outside = outside.trim();
    (!outside.is_empty()).then(|| outside.to_string())
}

/// Writes Link's block of file `path` as [`with_block`] says, the file's
/// mode 600 and the rest of it kept, unless the rest is `taken`: what
/// came out of it into the agent's spec, now in the block. A file that
/// would be empty is removed. Unchanged: nothing written. An error for a
/// link, or a file past [`TEXT_MAX`] twice over.
pub fn write_block(path: &Path, body: Option<&str>, taken: Option<&str>) -> anyhow::Result<()> {
    let old = read(path)?.unwrap_or_default();
    let came_out =
        outside_block(&old).is_some_and(|own| Some(own.as_str()) == taken.map(str::trim));
    let new = with_block(if came_out { "" } else { &old }, body);
    if new == old {
        return Ok(());
    }
    if new.is_empty() {
        return match std::fs::remove_file(path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
            _ => Ok(()),
        };
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    link_files::write_private(path, new.as_bytes())?;
    Ok(())
}

/// Writes `text` as file `path`'s own, what came in from another
/// harness: Link's block in it kept as it is.
pub fn write_outside(path: &Path, text: &str) -> anyhow::Result<()> {
    let old = read(path)?.unwrap_or_default();
    let block = old.find(BEGIN).map(|start| {
        let rest = &old[start + BEGIN.len()..];
        rest[..rest.find(END).unwrap_or(rest.len())]
            .trim()
            .to_string()
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    link_files::write_private(path, with_block(text, block.as_deref()).as_bytes())?;
    Ok(())
}

/// Writes Agent Skills folder `skill` under `root`: a name or a file's
/// path that would leave its folder is refused.
pub fn write_skill(
    root: &Path,
    skill: &link_agent_state_protocol::forms::Skill,
) -> anyhow::Result<()> {
    let plain = |path: &str| {
        let path = Path::new(path);
        !path.as_os_str().is_empty()
            && path
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
    };
    anyhow::ensure!(
        plain(&skill.name) && !skill.name.contains('/'),
        "skill {:?} isn't a folder's name",
        skill.name
    );
    let folder = root.join(&skill.name);
    for (path, text) in &skill.files {
        anyhow::ensure!(
            plain(path),
            "skill {}'s {path:?} leaves its folder",
            skill.name
        );
        let file = folder.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        link_files::write_private(&file, text.as_bytes())?;
    }
    Ok(())
}

/// File `path`'s text, never through a link; none when there's none.
pub fn read(path: &Path) -> anyhow::Result<Option<String>> {
    match std::fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
        Ok(meta) => anyhow::ensure!(
            meta.is_file() && meta.len() <= 2 * TEXT_MAX as u64,
            "{} isn't a file Link reads: a link, a folder or past {} bytes",
            path.display(),
            2 * TEXT_MAX
        ),
    }
    Ok(Some(
        String::from_utf8_lossy(&std::fs::read(path)?).into_owned(),
    ))
}

/// The folders under `root` holding a `SKILL.md`, at any depth, relative
/// to `home`; those named in `skip` (each folder's name) and anything
/// under a folder starting with `.` left out. No link is followed.
pub fn skill_folders(home: &Path, root: &Path, skip: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_dir() || name.starts_with('.') || skip.contains(&name) {
                continue;
            }
            let path = entry.path();
            if path.join(link_harness_wire::skill::FILE).is_file() {
                if let Ok(relative) = path.strip_prefix(home) {
                    found.push(relative.display().to_string());
                }
            } else if depth < 4 {
                stack.push((path, depth + 1));
            }
        }
    }
    found.sort();
    found
}

/// The most files of a skill given in portable form, and the most
/// bytes of each: what's past them, or isn't text, is left out.
pub const SKILL_FILES_MAX: usize = 200;
pub const SKILL_FILE_MAX: u64 = 1 << 20;

/// The skill in folder `dir` in ASP's portable form: its name as its
/// `SKILL.md` says, and its text files by their paths in it. None for a
/// folder whose `SKILL.md` isn't a skill's. No link is followed.
pub fn skill_form(dir: &Path) -> Option<link_agent_state_protocol::forms::Skill> {
    let skill =
        link_harness_wire::skill::parse(&read(&dir.join(link_harness_wire::skill::FILE)).ok()??)
            .ok()?;
    let mut files = std::collections::BTreeMap::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((folder, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let (path, Ok(kind)) = (entry.path(), entry.file_type()) else {
                continue;
            };
            if kind.is_dir() && depth < 8 {
                stack.push((path, depth + 1));
            } else if kind.is_file()
                && files.len() < SKILL_FILES_MAX
                && entry.metadata().is_ok_and(|m| m.len() <= SKILL_FILE_MAX)
                && let Ok(text) = String::from_utf8(std::fs::read(&path).unwrap_or_default())
                && let Ok(relative) = path.strip_prefix(dir)
            {
                files.insert(relative.display().to_string(), text);
            }
        }
    }
    Some(link_agent_state_protocol::forms::Skill {
        name: skill.name,
        files,
    })
}

/// File `path` as a note of the agent's memory, by `name`, its source
/// the harness's (`hermes:MEMORY.md`): none when it's missing or empty.
pub fn note(
    path: &Path,
    name: &str,
    adapter: &str,
) -> anyhow::Result<Option<link_agent_state_protocol::forms::Note>> {
    Ok(read(path)?.filter(|t| !t.trim().is_empty()).map(|text| {
        link_agent_state_protocol::forms::Note {
            path: name.to_string(),
            text,
            source: Some(format!("{adapter}:{name}")),
            deleted: false,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_goes_first_and_keeps_the_rest() {
        let text = with_block("Their own.\n", Some("Who you are: qa."));
        assert_eq!(
            text,
            format!("{BEGIN}\nWho you are: qa.\n{END}\n\nTheir own.\n")
        );
        assert_eq!(outside_block(&text).as_deref(), Some("Their own."));
    }

    #[test]
    fn a_block_is_replaced_in_place_and_taken_out() {
        let text = format!("Above.\n\n{BEGIN}\nold\n{END}\n\nBelow.\n");
        let replaced = with_block(&text, Some("new"));
        assert_eq!(
            replaced,
            format!("Above.\n\n{BEGIN}\nnew\n{END}\n\nBelow.\n")
        );
        assert_eq!(with_block(&replaced, None), "Above.\n\nBelow.\n");
        assert_eq!(with_block(&format!("{BEGIN}\nonly\n{END}\n"), None), "");
        assert_eq!(outside_block(&format!("{BEGIN}\nonly\n{END}\n")), None);
    }

    #[test]
    fn a_block_left_open_runs_to_the_end() {
        assert_eq!(
            with_block(&format!("Mine.\n{BEGIN}\nhalf"), None),
            "Mine.\n"
        );
    }

    #[test]
    fn the_texts_go_under_their_headings() {
        let identity = Identity {
            persona: Some("qa-persona".into()),
            owner: None,
            standing: Some(" \n".into()),
            skills: vec![],
        };
        assert_eq!(
            identity.who().as_deref(),
            Some("## Who you are\n\nqa-persona")
        );
        assert_eq!(identity.instructions(), None);
        assert_eq!(Identity::default().all(), None);
    }

    #[test]
    fn writing_a_block_keeps_the_file_and_removes_an_empty_one() {
        let dir = link_test_kit::TempDir::new("identity");
        let path = dir.path().join("SOUL.md");
        std::fs::write(&path, "Theirs.\n").unwrap();
        write_block(&path, Some("Ours."), None).unwrap();
        assert!(read(&path).unwrap().unwrap().ends_with("Theirs.\n"));
        write_block(&path, None, None).unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some("Theirs.\n"));
        let only = dir.path().join("AGENTS.md");
        write_block(&only, Some("Ours."), None).unwrap();
        write_block(&only, None, None).unwrap();
        assert!(!only.exists());
        std::fs::write(&path, "Call me Ada.\n").unwrap();
        write_block(
            &path,
            Some("## Who you are\n\nCall me Ada."),
            Some("Call me Ada.\n"),
        )
        .unwrap();
        assert_eq!(
            outside_block(&read(&path).unwrap().unwrap()),
            None,
            "taken, it isn't twice"
        );
    }

    #[test]
    fn a_link_is_never_read() {
        let dir = link_test_kit::TempDir::new("identity");
        std::fs::write(dir.path().join("elsewhere"), "secret").unwrap();
        std::os::unix::fs::symlink(dir.path().join("elsewhere"), dir.path().join("SOUL.md"))
            .unwrap();
        assert!(read(&dir.path().join("SOUL.md")).is_err());
        assert!(write_block(&dir.path().join("SOUL.md"), Some("x"), None).is_err());
    }

    #[test]
    fn an_agents_md_gives_back_the_texts_under_their_headings() {
        let who = Identity {
            persona: Some("qa-persona".into()),
            owner: Some("qa-owner".into()),
            standing: Some("qa-standing".into()),
            skills: vec![],
        };
        assert_eq!(Identity::of_agents_md(&who.all().unwrap()), who);
        let theirs = Identity::of_agents_md("# Rules\n\nBe kind.\n");
        assert_eq!(theirs.standing.as_deref(), Some("# Rules\n\nBe kind."));
        assert_eq!((theirs.persona, theirs.owner), (None, None));
    }

    #[test]
    fn a_skill_is_given_with_its_text_files() {
        let dir = link_test_kit::TempDir::new("identity-skill-form");
        assert_eq!(skill_form(&dir), None);
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: qa\ndescription: qa skill\n---\nSay qa.\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(dir.join("scripts/run.sh"), "echo qa\n").unwrap();
        std::fs::write(dir.join("logo.png"), [0xff, 0xfe, 0x00]).unwrap();
        let skill = skill_form(&dir).unwrap();
        assert_eq!(skill.name, "qa");
        assert_eq!(
            skill.files.keys().collect::<Vec<_>>(),
            ["SKILL.md", "scripts/run.sh"]
        );
    }

    #[test]
    fn skills_are_found_by_their_file_and_skipped_by_name() {
        let home = link_test_kit::TempDir::new("identity-skills");
        let root = home.path().join(".hermes/skills");
        for skill in ["notes/summarize", "plain", "bundled", ".hub/hidden"] {
            std::fs::create_dir_all(root.join(skill)).unwrap();
            std::fs::write(root.join(skill).join("SKILL.md"), "---\n").unwrap();
        }
        assert_eq!(
            skill_folders(home.path(), &root, &["bundled".into()]),
            [".hermes/skills/notes/summarize", ".hermes/skills/plain"]
        );
    }
}
