//! A conversation's chain: how many agents' messages it took since a
//! person last spoke in it. A person's message ends the chain; past the
//! receiving agent's `[tool.scheduler] hops`, another agent's message is
//! refused ([`Capped`]). It holds on a chat a connection carries, where a
//! person reads along; a conversation only agents write in, where no
//! person ever speaks, is bounded by each message's own hops instead.
//!
//! Kept on the machine beside the interactions' records, a file for each
//! conversation, changed under a lock, so a connection's bridge
//! and the agent's way in, each its own process, count one chain.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// The folder of the chains, in the interactions' folder.
const CHAINS: &str = "chains";

/// One conversation's chain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chain {
    /// Agents' messages taken since a person last spoke.
    pub agents: u32,
}

/// Another agent's message refused at the cap: `agents` were taken since
/// a person last spoke. It stays refused until a person speaks, so it's
/// never tried again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capped {
    pub agents: u32,
}

impl std::fmt::Display for Capped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} agents' messages in this conversation since a person last spoke in it, the \
             cap ([tool.scheduler] hops): not taken until a person speaks",
            self.agents
        )
    }
}

impl std::error::Error for Capped {}

/// The chains of a machine's conversations.
#[derive(Clone, Debug)]
pub struct Chains {
    dir: PathBuf,
}

impl Chains {
    /// This machine's, beside its interactions' records.
    pub fn machine() -> anyhow::Result<Chains> {
        Ok(Chains::at(
            &link_files::layout::Hidden::Approvals.try_path()?,
        ))
    }

    /// Those kept in interactions' folder `dir`.
    pub fn at(dir: &Path) -> Chains {
        Chains {
            dir: dir.join(CHAINS),
        }
    }

    /// Agent `agent`'s (its ID) conversation `thread`'s chain now.
    pub fn of(&self, agent: &str, thread: &str) -> anyhow::Result<Chain> {
        let (path, _lock) = self.locked(agent, thread)?;
        read(&path)
    }

    /// A person spoke in it: its chain ends.
    pub fn person_spoke(&self, agent: &str, thread: &str) -> anyhow::Result<()> {
        let (path, _lock) = self.locked(agent, thread)?;
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(e).with_context(|| format!("removing {}", path.display()))
            }
            _ => Ok(()),
        }
    }

    /// Another agent's message is taken in it, refused once `hops`
    /// were since a person last spoke ([`Capped`]): how many there are
    /// with it.
    pub fn agent_said(&self, agent: &str, thread: &str, hops: u32) -> anyhow::Result<u32> {
        let (path, _lock) = self.locked(agent, thread)?;
        let mut chain = read(&path)?;
        if chain.agents >= hops {
            return Err(Capped {
                agents: chain.agents,
            }
            .into());
        }
        chain.agents += 1;
        write(&path, &chain)?;
        Ok(chain.agents)
    }

    /// Undoes `agent_said` for a message never taken.
    pub fn unsay(&self, agent: &str, thread: &str) -> anyhow::Result<()> {
        let (path, _lock) = self.locked(agent, thread)?;
        let mut chain = read(&path)?;
        chain.agents = chain.agents.saturating_sub(1);
        write(&path, &chain)
    }

    /// The chain's file, and its lock, held.
    fn locked(&self, owner: &str, thread: &str) -> anyhow::Result<(PathBuf, std::fs::File)> {
        anyhow::ensure!(
            !owner.is_empty()
                && owner
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{:?} isn't an agent's ID",
            link_format::clean(owner)
        );
        let dir = self.dir.join(owner);
        link_files::create_private_dir(&dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let name = link_digest::sha256(thread.as_bytes());
        let lock = link_files::lock_file(&dir.join(format!("{name}.lock")))?;
        Ok((dir.join(format!("{name}.json")), lock))
    }
}

link_files::specification!(Chain, include_str!("chains.schema.toml"));

/// Each thread's approval chain, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Chain>()
}

fn read(path: &Path) -> anyhow::Result<Chain> {
    match std::fs::read(path) {
        Ok(bytes) => link_files::saved::parse_json(&bytes)
            .with_context(|| format!("reading {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Chain::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn write(path: &Path, chain: &Chain) -> anyhow::Result<()> {
    link_files::write_private(path, &link_files::saved::print_json(chain)?)
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_messages_are_capped_until_a_person_speaks() {
        let dir = link_test_kit::TempDir::new("chains");
        let chains = Chains::at(&dir);
        let (me, team) = ("aaaaaaaaaaaa", "team-r7");
        assert_eq!(chains.agent_said(me, team, 2).unwrap(), 1);
        assert_eq!(chains.agent_said(me, team, 2).unwrap(), 2);
        let refused = chains.agent_said(me, team, 2).unwrap_err();
        assert!(refused.to_string().contains("until a person speaks"));
        assert_eq!(
            refused.downcast_ref::<Capped>(),
            Some(&Capped { agents: 2 })
        );
        // Another conversation, another agent: their own chains.
        assert_eq!(chains.agent_said(me, "slack:C1", 2).unwrap(), 1);
        assert_eq!(chains.agent_said("bbbbbbbbbbbb", team, 2).unwrap(), 1);
        // One never taken is given back.
        chains.unsay(me, "slack:C1").unwrap();
        assert_eq!(chains.of(me, "slack:C1").unwrap().agents, 0);
        // A person speaks: the chain starts again.
        chains.person_spoke(me, team).unwrap();
        assert_eq!(chains.of(me, team).unwrap(), Chain::default());
        assert_eq!(chains.agent_said(me, team, 2).unwrap(), 1);
        assert!(chains.agent_said("../x", team, 2).is_err());
        // With no hops at all, no agent's message is taken.
        assert!(chains.agent_said(me, "team-q1", 0).is_err());
    }

    #[test]
    fn concurrent_agents_never_pass_the_cap() {
        let dir = link_test_kit::TempDir::new("chains-race");
        let chains = Chains::at(&dir);
        let taken = std::thread::scope(|s| {
            let tries: Vec<_> = (0..8)
                .map(|_| s.spawn(|| chains.agent_said("aaaaaaaaaaaa", "team-r7", 3).is_ok()))
                .collect();
            tries
                .into_iter()
                .filter_map(|t| t.join().ok())
                .filter(|ok| *ok)
                .count()
        });
        assert_eq!(taken, 3);
    }
}
