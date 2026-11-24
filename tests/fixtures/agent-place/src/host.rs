//! Hosted state lives in `agents/<id>/`: `agent.json`, a move manifest,
//! and the `files/`, `memory/`, `harness/` and `connections/` folders.
//! `backups/<id>/` holds the last file backup and its timestamp, `at`.
//!
//! Fixture controls: `silent-agent-list` or `no-agents` omits the inventory;
//! `no-backups` makes backups unsupported. `teams.json` and `teams/<id>.toml`
//! supply read-only team answers. `connected.json` records a connection.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use link_agent_contract::moving;
use link_agent_place::{HostedAgent, Reply, Request};
use serde_json::{Value, json};

pub(crate) struct Host {
    pub(crate) dir: PathBuf,
}

pub(crate) fn read_json(path: &Path) -> anyhow::Result<Value> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub(crate) fn write_json(path: &Path, value: &Value) -> anyhow::Result<()> {
    std::fs::create_dir_all(path.parent().context("no folder")?)?;
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

impl Host {
    pub(crate) fn connect(&self, request: &Request) -> anyhow::Result<Reply> {
        let agent = request.agent.as_ref().context("no agent")?;
        ensure!(self.agent_dir(&agent.id)?.is_dir(), "no such hosted agent");
        let connect = request.connect.as_ref().context("no connection")?;
        write_json(
            &self.dir.join("connected.json"),
            &json!({
                "agent":agent.id,"connection":connect.connection,"token":connect.token.expose(),"user":connect.user
            }),
        )?;
        Ok(Reply {
            message: Some("Telegram connected".into()),
            ..Reply::default()
        })
    }

    pub(crate) fn create(&self, request: &Request) -> anyhow::Result<Reply> {
        let name = &request.create.as_ref().context("no create")?.name;
        link_machine::machine::check_name(name)?;
        let agent = HostedAgent {
            id: link_id::candidate()?,
            name: name.clone(),
            state: Some("running".into()),
            ..Default::default()
        };
        let dir = self.agent_dir(&agent.id)?;
        std::fs::create_dir_all(dir.join("files"))?;
        write_json(&dir.join("agent.json"), &serde_json::to_value(&agent)?)?;
        write_json(
            &dir.join("manifest.json"),
            &json!({
                "format":moving::PROTOCOL.current,"phase":"switch","move":moving::new_move_id()?,
                "agent":{"workload":format!("agent:{}", agent.id),"name":name},"harness":"link",
                "model":{"provider":"openai","name":"managed/default","api_key":""},"removed":[]
            }),
        )?;
        Ok(Reply {
            agent: Some(agent),
            ..Default::default()
        })
    }

    pub(crate) fn agent_dir(&self, id: &str) -> anyhow::Result<PathBuf> {
        link_machine::machine::check_id(id)?;
        Ok(self.dir.join("agents").join(id))
    }

    pub(crate) fn staging(&self, id: &str) -> anyhow::Result<PathBuf> {
        link_machine::machine::check_id(id)?;
        Ok(self.dir.join("staging").join(id))
    }

    pub(crate) fn spool(&self) -> PathBuf {
        self.dir.join("spool")
    }

    pub(crate) fn list(&self, owner: Option<&str>) -> anyhow::Result<Reply> {
        // A reply that doesn't say, as a test asks.
        if self.dir.join("no-agents").exists() {
            return Ok(Reply::default());
        }
        let mut agents = Vec::new();
        for entry in std::fs::read_dir(self.dir.join("agents"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let Ok(agent) = read_json(&entry.path().join("agent.json")) else {
                continue;
            };
            let agent: HostedAgent = serde_json::from_value(agent)?;
            if owner.is_none() || agent.owner.as_deref() == owner || agent.owner.is_none() {
                agents.push(agent);
            }
        }
        Ok(Reply {
            agents: Some(agents),
            ..Reply::default()
        })
    }

    /// `agent-backup`: the agent's files copied aside, with when; with
    /// `restore`, put back in their place.
    pub(crate) fn backup(&self, request: &Request, restore: bool) -> anyhow::Result<Reply> {
        if self.dir.join("no-backups").exists() {
            // As `link_agent_place::NoCommand` reads it.
            bail!("lnk-agent-place-fixture has no command \"agent-backup\"");
        }
        let id = &request.agent.as_ref().context("no agent")?.id;
        let agent = self.agent_dir(id)?;
        ensure!(agent.join("agent.json").exists(), "no such agent here");
        let kept = self.dir.join("backups").join(id);
        let at = match restore {
            false => {
                let _ = std::fs::remove_dir_all(&kept);
                std::fs::create_dir_all(&kept)?;
                copy_folder(&agent.join("files"), &kept.join("files"))?;
                let at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs();
                std::fs::write(kept.join("at"), at.to_string())?;
                at
            }
            true => {
                let at: u64 = std::fs::read_to_string(kept.join("at"))
                    .context("no backup of that agent here")?
                    .trim()
                    .parse()?;
                let _ = std::fs::remove_dir_all(agent.join("files"));
                copy_folder(&kept.join("files"), &agent.join("files"))?;
                at
            }
        };
        Ok(Reply {
            at: Some(at),
            ..Reply::default()
        })
    }

    /// `agent-team`'s list and each team's file, as the test wrote them:
    /// `teams.json` (a list of `HostedTeam`) and `teams/<id>.toml`.
    pub(crate) fn team(&self, request: &Request) -> anyhow::Result<Reply> {
        use link_agent_place::teams::{Action, Reply as TeamReply, TeamFile};
        let team = match request.team.as_ref().context("no team action")? {
            Action::List(_) => TeamReply {
                teams: Some(serde_json::from_value(read_json(
                    &self.dir.join("teams.json"),
                )?)?),
                ..TeamReply::default()
            },
            Action::Get(link_agent_place::teams::TeamActionGet { team }) => {
                link_machine::machine::check_id(team)?;
                let path = self.dir.join("teams").join(format!("{team}.toml"));
                TeamReply {
                    file: Some(TeamFile {
                        file: std::fs::read_to_string(&path)
                            .with_context(|| format!("reading {}", path.display()))?,
                        version: 1,
                        joined: Vec::new(),
                    }),
                    ..TeamReply::default()
                }
            }
            other => bail!("lnk-agent-place-fixture keeps no team for {other:?}"),
        };
        Ok(Reply {
            team: Some(team),
            ..Reply::default()
        })
    }
}

/// The bytes of the files under `path`.
pub(crate) fn size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| size(&e.path()))
        .sum()
}

/// Folder `from`'s files and folders, copied to `to`; links left out.
fn copy_folder(from: &Path, to: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
