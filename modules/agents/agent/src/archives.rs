//! An agent's answers to the archive's questions
//! ([`link_archive_contract`]), each from files alone: its files read by
//! their own types, whether it runs, what it needs here, and a new agent
//! adopted from files that arrived. Only an agent on Link Harness is
//! carried, since another harness keeps its own state in its home.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use link_archive_contract::{
    Adopt, Adopted, Carried, Carry, Checked, Missing, Need, Replaced, Running, Thing,
};
use link_files::layout::{Home, map::Entry};
use link_files::nofollow::{Dir, Kind};
use link_store_contract::store::compact::FRAMES;
use link_store_contract::store::folder::{Conversation, EVENTS};

use crate::settings::{self, DEFAULT_HARNESS, Settings};

/// The kind an archive selects agents by, as `layout.toml` names it.
pub const KIND: &str = "agent";

/// `thing`'s folder of location `home`.
fn folder(thing: &Thing, home: Home) -> anyhow::Result<&PathBuf> {
    ensure!(
        thing.kind == KIND,
        "{} isn't an agent: this module carries agents",
        thing.kind
    );
    thing
        .folders
        .get(&Entry::Home(home).name())
        .with_context(|| {
            format!(
                "no folder of {} for {}",
                Entry::Home(home).name(),
                thing.name
            )
        })
}

/// Its settings, read strictly from its folder: none of this machine's
/// records, nor a key it refuses.
fn settings_in(dir: &Path, name: &str) -> anyhow::Result<(Settings, String)> {
    let path = dir.join(settings::SETTINGS_FILE);
    let text =
        link_files::fs::read_control(&path, link_spec::MAX_BYTES, "an agent's settings'")?.text;
    let mut settings: Settings = link_files::saved::parse(&text)
        .with_context(|| format!("reading {}'s {}", name, settings::SETTINGS_FILE))?;
    settings::configuration::behavior_text(&text)
        .with_context(|| format!("reading {}'s spec", name))?;
    settings.name = name.to_string();
    Ok((settings, text))
}

/// The files an agent's folder in `~/Link/Safe/Agents` holds: anything
/// else there is refused, since it would be control Link never asked
/// for.
const CONTROL_FILES: [&str; 2] = [settings::SETTINGS_FILE, "sandbox.toml"];

/// `archive.check`: its settings, sandbox file and skills read with
/// their own types, nothing else in its settings' folder, and refused on
/// another harness.
pub fn check(thing: &Thing) -> anyhow::Result<Checked> {
    let control = folder(thing, Home::AgentSettings)?;
    for entry in
        std::fs::read_dir(control).with_context(|| format!("reading {}", control.display()))?
    {
        let name = entry?.file_name();
        // A system's own, such as a Mac's `.DS_Store`, which Link never
        // reads.
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        ensure!(
            CONTROL_FILES.iter().any(|known| name == *known),
            "{}'s settings folder holds {}, which no agent's does: only {}",
            thing.name,
            name.to_string_lossy(),
            CONTROL_FILES.join(" and ")
        );
    }
    let (settings, _) = settings_in(control, &thing.name)?;
    if settings.harness() != DEFAULT_HARNESS {
        bail!(
            "{} runs on the {} harness, which keeps its own state in its home: an archive carries \
             only an agent on Link Harness, so lnk agent harness set link --agent {} brings its \
             chats and memory to Link Harness first",
            thing.name,
            settings.harness(),
            thing.name
        );
    }
    let sandbox = folder(thing, Home::AgentSettings)?.join("sandbox.toml");
    if let Some(read) =
        link_files::fs::read_control_optional(&sandbox, link_spec::MAX_BYTES, "an agent's grants'")?
    {
        crate::sandbox_file::Sandbox::parse(&read.text)
            .with_context(|| format!("reading {}'s sandbox.toml", thing.name))?;
    }
    let own = link_agent_contract::folder::Folder::new(folder(thing, Home::Agents)?, None);
    crate::skills::list(&own.agent).with_context(|| format!("reading {}'s skills", thing.name))?;
    Ok(Checked {})
}

/// `archive.running`: whether it runs here, as its service or in a
/// terminal.
pub fn running(thing: &Thing) -> anyhow::Result<Running> {
    folder(thing, Home::AgentSettings)?;
    Ok(Running {
        stop: crate::service::running_anywhere(&thing.name)
            .then(|| format!("lnk agent stop --agent {}", thing.name)),
    })
}

/// `archive.carry`: its conversations without this machine's identity
/// of the work each model request and proposed call was part of (an
/// evidence record's `work`, naming the agent's ID, its session and its
/// attempt here), which nothing reading a conversation needs. Each
/// conversation holding one goes as a log of its events, each record's
/// `work` none, and a compact one's frames stay behind, its events in
/// that log. The rest of its files go as they are.
pub fn carry(asked: &Carry) -> anyhow::Result<Carried> {
    let own = link_agent_contract::folder::Folder::new(folder(&asked.thing, Home::Agents)?, None);
    let chats = own.agent.join(link_harness_base::layout::CONVERSATIONS);
    let dir = match Dir::open_unlinked(&chats) {
        Ok(dir) => dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Carried::default()),
        Err(err) => return Err(err).with_context(|| format!("reading {}", chats.display())),
    };
    let mut replaced = Vec::new();
    for name in dir
        .names()
        .with_context(|| format!("reading {}", chats.display()))?
    {
        let shown = chats.join(&name);
        let Some(text) = name.to_str().filter(|n| !n.starts_with('.')) else {
            continue;
        };
        if dir.kind(&name)? != Kind::Folder {
            continue;
        }
        let conv = dir
            .folder(&name)
            .with_context(|| format!("reading {}", shown.display()))?;
        let mut files = BTreeMap::new();
        for file in [EVENTS, FRAMES] {
            match conv.open_entry(OsStr::new(file)) {
                Ok(open) => {
                    let bytes =
                        link_files::nofollow::read_open(open, link_archive::limits().max_bytes)
                            .with_context(|| format!("reading {}", shown.join(file).display()))?;
                    files.insert(file.to_string(), bytes);
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("reading {}", shown.join(file).display()));
                }
            }
        }
        let Some(conversation) = Conversation::from_files(text, &files)? else {
            continue;
        };
        let mut changed = false;
        let mut log = String::new();
        for line in &conversation.events {
            let carried = machine_neutral(line)
                .with_context(|| format!("reading an event of {}", shown.display()))?;
            changed |= carried.is_some();
            log.push_str(carried.as_deref().unwrap_or(line));
            log.push('\n');
        }
        if !changed {
            continue;
        }
        let with = asked.staging.join(text).join(EVENTS);
        std::fs::create_dir_all(with.parent().context("a staged log has a folder")?)?;
        std::fs::write(&with, log).with_context(|| format!("writing {}", with.display()))?;
        replaced.push(Replaced {
            file: shown.join(EVENTS),
            with: Some(with),
        });
        if files.contains_key(FRAMES) {
            replaced.push(Replaced {
                file: shown.join(FRAMES),
                with: None,
            });
        }
    }
    Ok(Carried { replaced })
}

/// Event line `line` with its evidence record's `work` none, seq first
/// as the log writes it: none when it holds no such record, or its
/// record names no work.
fn machine_neutral(line: &str) -> anyhow::Result<Option<String>> {
    use link_harness_wire::evidence::{Evidence, Record};
    let mut event: serde_json::Map<String, serde_json::Value> = serde_json::from_str(line)?;
    let Some(raw) = event.get_mut("evidence") else {
        return Ok(None);
    };
    let mut evidence: Evidence = serde_json::from_value(raw.clone())?;
    let work = match &mut evidence.record {
        Record::ModelRequested { request } => &mut request.work,
        Record::Proposed(proposal) => &mut proposal.work,
        Record::Dispatched { .. } | Record::Finished { .. } | Record::SkillLoaded { .. } => {
            return Ok(None);
        }
    };
    if work.take().is_none() {
        return Ok(None);
    }
    *raw = serde_json::to_value(&evidence)?;
    let seq = event.remove("seq").context("an event without its number")?;
    let rest = serde_json::to_string(&event)?;
    Ok(Some(match rest.as_str() {
        "{}" => format!("{{\"seq\":{seq}}}"),
        _ => format!("{{\"seq\":{seq},{}", &rest[1..]),
    }))
}

/// `archive.missing`: its model's credential, which its first start
/// asks for, and each connection, whose secrets stayed behind.
pub fn missing(thing: &Thing) -> anyhow::Result<Missing> {
    let (settings, _) = settings_in(folder(thing, Home::AgentSettings)?, &thing.name)?;
    let start = format!("lnk agent start --agent {}", thing.name);
    let mut needs = Vec::new();
    match &settings.model {
        None => needs.push(Need {
            what: "A model".into(),
            setup: start.clone(),
        }),
        Some(model) if settings::key_var(&model.provider).is_some() => needs.push(Need {
            what: format!("Its model provider's credential ({})", model.provider),
            setup: start.clone(),
        }),
        Some(_) => {}
    }
    for connection in settings.connections() {
        needs.push(Need {
            what: format!("Its {connection} connection"),
            setup: format!("lnk agent connect {connection} --agent {}", thing.name),
        });
    }
    Ok(Missing { needs })
}

/// `archive.adopt`: a new agent, with an ID of its own, from the files
/// that arrived in its folders.
pub fn adopt(asked: &Adopt) -> anyhow::Result<Adopted> {
    let settings_dir = folder(&asked.thing, Home::AgentSettings)?;
    ensure!(
        settings_dir == &settings::configuration_of(&asked.thing.name),
        "{} isn't where agent {}'s settings are here",
        settings_dir.display(),
        asked.thing.name
    );
    // What it was granted where it came from, said before it starts.
    let beyond =
        crate::sandbox_file::beyond_defaults(&crate::sandbox_file::load(&asked.thing.name)?);
    let (_, mut notes) = Settings::adopt(&asked.thing.name, &asked.was)?;
    if !beyond.is_empty() {
        notes.push(format!(
            "Its sandbox file opens more than Link's does: {}. lnk agent permission remove --agent {} takes \
             each back before it starts",
            beyond.join(", "),
            asked.thing.name
        ));
    }
    Ok(Adopted {
        notes,
        next: format!("lnk agent start --agent {}", asked.thing.name),
    })
}

#[cfg(test)]
mod tests;
