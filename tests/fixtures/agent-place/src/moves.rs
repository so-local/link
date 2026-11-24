//! Move state is separate from hosted state. `moves/<move>.json` records
//! direction, agent and recipient; `staging/<id>/` receives a move in.
//! Copy transfers files while the source answers. Switch seals private
//! state, pauses the source and promotes the destination's staging folder.
//!
//! Fixture controls: `refuse.json` refuses entry, `managed-models` advertises
//! managed models, and `held.json` supplies messages handed over on leaving.

use std::io::{BufRead, Write};

use anyhow::{Context, ensure};
use link_agent_contract::moving::{self, Have, Phase};
use link_agent_place::{self as cloud, HostedAgent, Reply, Request};
use serde_json::{Value, json};

use crate::host::{Host, read_json, size, write_json};

impl Host {
    /// The move `request` names, one it started: which way (`in`,
    /// `out`), and whose.
    fn the_move(&self, request: &Request) -> anyhow::Result<(String, HostedAgent)> {
        let id = request.r#move.as_deref().context("no move")?;
        moving::check_move_id(id)?;
        let record = read_json(&self.dir.join("moves").join(format!("{id}.json")))?;
        let agent: HostedAgent = serde_json::from_value(record["agent"].clone())?;
        Ok((record["way"].as_str().unwrap_or_default().into(), agent))
    }

    fn new_move(&self, way: &str, agent: &HostedAgent) -> anyhow::Result<String> {
        let id = moving::new_move_id()?;
        let recipient = (way == "in").then(link_secretive::Recipient::new);
        write_json(
            &self.dir.join("moves").join(format!("{id}.json")),
            &json!({"way": way, "agent": agent,
                "recipient": recipient.map(|r| r.keep().expose().clone())}),
        )?;
        Ok(id)
    }

    fn recipient(&self, id: &str) -> anyhow::Result<link_secretive::Recipient> {
        moving::check_move_id(id)?;
        let record = read_json(&self.dir.join("moves").join(format!("{id}.json")))?;
        let kept = link_secretive::Secret::new(
            record["recipient"]
                .as_str()
                .context("no receiver")?
                .to_owned(),
        );
        link_secretive::Recipient::from_kept(&kept).context("invalid receiver")
    }

    /// Accept an incoming move, reporting staged files and a sealing recipient.
    pub(crate) fn move_in(&self, request: &Request) -> anyhow::Result<Reply> {
        let agent = request.agent.clone().context("no agent")?;
        if let Ok(refused) = read_json(&self.dir.join("refuse.json")) {
            return Ok(Reply {
                refused: Some(serde_json::from_value(refused)?),
                ..Reply::default()
            });
        }
        let staging = self.staging(&agent.id)?;
        let mut have = moving::have_of(&staging.join("files"), "files")?;
        have.extend(moving::have_of(&staging.join("memory"), "memory")?);
        let id = self.new_move("in", &agent)?;
        Ok(Reply {
            seal_to: Some(self.recipient(&id)?.seal_to().clone()),
            managed_models: self.dir.join("managed-models").exists().then_some(true),
            r#move: Some(id),
            have,
            ..Reply::default()
        })
    }

    /// Start an outgoing move while the agent still answers here.
    pub(crate) fn move_out(&self, request: &Request) -> anyhow::Result<Reply> {
        let named = request.agent.clone().context("no agent")?;
        let dir = self.agent_dir(&named.id)?;
        let agent: HostedAgent = serde_json::from_value(read_json(&dir.join("agent.json"))?)
            .context("no such agent here")?;
        Ok(Reply {
            r#move: Some(self.new_move("out", &agent)?),
            size: Some(size(&dir.join("files"))),
            ..Reply::default()
        })
    }

    /// Receive files into staging. Only a sealed switch promotes the agent
    /// to hosted state; a plain file transfer updates an already hosted agent.
    pub(crate) fn take(
        &self,
        request: &Request,
        input: &mut impl BufRead,
    ) -> anyhow::Result<Reply> {
        let phase = Phase::of(request.phase.as_deref().unwrap_or_default())?;
        if phase == Phase::Files {
            let agent = request.agent.clone().context("no agent")?;
            let dir = self.agent_dir(&agent.id)?;
            ensure!(dir.join("agent.json").exists(), "no such agent here");
            // What Home holds, first; then what's new.
            let have = moving::have_of(&dir.join("files"), "files")?;
            let first = Reply {
                have,
                ..Reply::default()
            };
            println!("{}", serde_json::to_string(&first)?);
            std::io::stdout().flush()?;
            moving::unpack(input, |_, head| {
                ensure!(head.phase == Phase::Files, "not a stream of files");
                Ok(dir.clone())
            })?;
            return Ok(Reply::default());
        }
        let (way, agent) = self.the_move(request)?;
        ensure!(way == "in", "that move isn't one in");
        let staging = self.staging(&agent.id)?;
        let mut manifest = Value::Null;
        let input: Box<dyn std::io::Read + '_> = if phase == Phase::Switch {
            Box::new(link_secretive::stream::Receiver::new(
                input,
                &self.recipient(request.r#move.as_deref().context("no move")?)?,
                &moving::stream_about(),
            )?)
        } else {
            Box::new(input)
        };
        moving::unpack(input, |value, head| {
            ensure!(head.phase == phase, "the stream isn't the move's {phase:?}");
            ensure!(
                Some(head.move_id.as_str()) == request.r#move.as_deref(),
                "the stream is of another move"
            );
            ensure!(
                value["agent"]["workload"] == format!("agent:{}", agent.id).as_str(),
                "another agent's"
            );
            if phase == Phase::Switch {
                for folder in ["harness", "connections", "schedule"] {
                    let _ = std::fs::remove_dir_all(staging.join(folder));
                }
                for removed in &head.removed {
                    moving::remove_under(&staging, removed)?;
                }
            }
            manifest = value.clone();
            Ok(staging.clone())
        })?;
        if phase == Phase::Copy {
            let mut have = moving::have_of(&staging.join("files"), "files")?;
            have.extend(moving::have_of(&staging.join("memory"), "memory")?);
            return Ok(Reply {
                have,
                ..Reply::default()
            });
        }
        // The switch: the agent answers here now.
        let dir = self.agent_dir(&agent.id)?;
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.parent().context("no folder")?)?;
        std::fs::rename(&staging, &dir)?;
        write_json(&dir.join("manifest.json"), &manifest)?;
        let hosted = HostedAgent {
            state: Some("running".into()),
            ..agent
        };
        write_json(&dir.join("agent.json"), &serde_json::to_value(&hosted)?)?;
        Ok(Reply {
            answers: Some(true),
            machine: Some("agent-place-fixture-1".into()),
            ..Reply::default()
        })
    }

    /// Send changed files. A switch pauses the agent and seals private state
    /// to the destination's recipient; copy and file transfers are unsealed.
    pub(crate) fn give(&self, request: &Request) -> anyhow::Result<()> {
        let phase = Phase::of(request.phase.as_deref().unwrap_or_default())?;
        let have: Have = request.have.clone();
        moving::check_have(&have)?;
        let (agent, move_id) = match phase {
            Phase::Files => (
                request.agent.clone().context("no agent")?,
                moving::new_move_id()?,
            ),
            _ => {
                let (way, agent) = self.the_move(request)?;
                ensure!(way == "out", "that move isn't one out");
                (agent, request.r#move.clone().unwrap_or_default())
            }
        };
        let dir = self.agent_dir(&agent.id)?;
        let stored = read_json(&dir.join("manifest.json"))?;
        if phase == Phase::Switch {
            let paused = HostedAgent {
                state: Some("paused".into()),
                ..agent.clone()
            };
            write_json(&dir.join("agent.json"), &serde_json::to_value(&paused)?)?;
        }
        let mut here = moving::have_of(&dir.join("files"), "files")?;
        here.extend(moving::have_of(&dir.join("memory"), "memory")?);
        let removed: Vec<&String> = have.keys().filter(|p| !here.contains_key(*p)).collect();
        let manifest = match phase {
            Phase::Switch => {
                let mut manifest = stored;
                manifest["phase"] = json!("switch");
                manifest["move"] = json!(move_id);
                manifest["removed"] = json!(removed);
                manifest
            }
            _ => json!({
                "format": moving::PROTOCOL.current, "phase": phase, "move": move_id,
                "version": stored["version"], "agent": stored["agent"],
                "harness": stored["harness"],
            }),
        };
        let write = |out: &mut dyn Write| -> anyhow::Result<()> {
            let mut tar = tar::Builder::new(out);
            tar.follow_symlinks(false);
            moving::append_manifest(&mut tar, &serde_json::to_vec(&manifest)?)?;
            for folder in phase.folders() {
                if dir.join(folder).is_dir() {
                    moving::append_folder(
                        &mut tar,
                        &dir.join(folder),
                        folder,
                        &have,
                        None,
                        &self.spool(),
                    )?;
                }
            }
            moving::append_end(&mut tar)?;
            tar.into_inner()?.flush()?;
            Ok(())
        };
        let mut out = std::io::BufWriter::new(std::io::stdout().lock());
        if phase == Phase::Switch {
            let to = request.seal_to.as_ref().context("no receiver")?;
            let mut sealed = link_secretive::stream::Sender::new(out, to, moving::stream_about())?;
            write(&mut sealed)?;
            sealed.finish()?;
        } else {
            write(&mut out)?;
        }
        Ok(())
    }

    /// Finish an outgoing move and hand over messages held during the switch.
    pub(crate) fn left(&self, request: &Request) -> anyhow::Result<Reply> {
        let (way, agent) = self.the_move(request)?;
        ensure!(way == "out", "that move isn't one out");
        std::fs::remove_dir_all(self.agent_dir(&agent.id)?)?;
        let mut held: Vec<cloud::Held> = match read_json(&self.dir.join("held.json")) {
            Ok(held) => serde_json::from_value(held)?,
            Err(_) => Vec::new(),
        };
        // Each held for the generation this move makes, as a cloud that
        // hosts agents names it, unless the test named another.
        for message in &mut held {
            if message.generation.is_none() {
                message.generation = request.r#move.clone();
            }
        }
        let _ = std::fs::remove_file(self.dir.join("held.json"));
        Ok(Reply {
            held,
            ..Reply::default()
        })
    }

    /// Discard incoming staging, or resume the source of an outgoing move.
    pub(crate) fn abort(&self, request: &Request) -> anyhow::Result<Reply> {
        let (way, agent) = self.the_move(request)?;
        match way.as_str() {
            "in" => {
                let _ = std::fs::remove_dir_all(self.staging(&agent.id)?);
            }
            _ => {
                let dir = self.agent_dir(&agent.id)?;
                if dir.join("agent.json").exists() {
                    let running = HostedAgent {
                        state: Some("running".into()),
                        ..agent
                    };
                    write_json(&dir.join("agent.json"), &serde_json::to_value(&running)?)?;
                }
            }
        }
        Ok(Reply::default())
    }
}
