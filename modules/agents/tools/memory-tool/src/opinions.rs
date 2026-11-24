//! What an agent's memory may spend ([`Opinions`]): its specification's
//! `[tool.memory]` keys, each a `serve` flag, and Link's specification's
//! where neither says. What makes memory correct and safe stays a
//! decision, in `limits.toml`.

use anyhow::{Context, Result};

use crate::cli::MemoryToolServeArgs;

/// A folder of the agent's that memory indexes, as `serve --folders`
/// declares it: its Home's `memory/`, or its conversations, `chats/`.
pub use crate::cli::MemoryToolServeArgsFoldersItem as Indexed;

impl Indexed {
    /// Its name, as a specification and the folder say it.
    pub fn as_str(self) -> &'static str {
        match self {
            Indexed::Memory => link_harness_contract::harness::layout::MEMORY,
            Indexed::Chats => crate::CONVERSATIONS,
        }
    }

    /// The folder a specification's `folders` names.
    fn of(folder: link_agent_spec::declared::AgentToolFoldersItem) -> Indexed {
        use link_agent_spec::declared::AgentToolFoldersItem as Named;
        match folder {
            Named::Memory => Indexed::Memory,
            Named::Chats => Indexed::Chats,
        }
    }
}

/// What an agent's memory may spend, as its specification chooses.
#[derive(Clone, Debug, PartialEq)]
pub struct Opinions {
    /// The most the service holds of its own for the index, in bytes,
    /// besides mapped pages: SQLite's page cache, each connection's.
    pub memory: u64,
    /// The disk left free, in bytes: indexing pauses below it.
    pub reserve: u64,
    /// The largest file indexed, in bytes.
    pub file: u64,
    /// The threads indexing takes, across the service.
    pub threads: usize,
    /// How long a search ranks before it answers with what it has.
    pub deadline: std::time::Duration,
    /// How many of the model's dimensions are kept.
    pub dimensions: u32,
    /// The agent's folders it indexes.
    pub folders: Vec<Indexed>,
    /// The largest index kept off the machine, in bytes.
    pub kept_off: u64,
}

impl Default for Opinions {
    /// Link's specification's.
    fn default() -> Self {
        Self::link().clone()
    }
}

impl Opinions {
    /// Link's specification's `[tool.memory]`, read once.
    pub fn link() -> &'static Opinions {
        static LINK: std::sync::LazyLock<Opinions> = std::sync::LazyLock::new(|| {
            Opinions::of(&link_agent_spec::link().tool("memory"), None)
                .expect("Link's specification holds memory's opinions")
        });
        &LINK
    }

    /// `tool`'s, a specification's `[tool.memory]`, each key it leaves
    /// out `under`'s, else Link's specification's.
    pub fn of(tool: &link_agent_spec::Tool, under: Option<&Opinions>) -> Result<Opinions> {
        let size = |key: &str, given: &Option<String>, under: Option<u64>| -> Result<u64> {
            match (given, under) {
                (Some(given), _) => link_agent_spec::size(given).with_context(|| key.to_string()),
                (None, Some(under)) => Ok(under),
                (None, None) => anyhow::bail!("Link's specification sets no {key}"),
            }
        };
        let folders = match (&tool.folders, under) {
            (Some(folders), _) => folders.iter().copied().map(Indexed::of).collect(),
            (None, Some(under)) => under.folders.clone(),
            (None, None) => anyhow::bail!("Link's specification sets no folders"),
        };
        let number = |key: &str, given: Option<u64>, under: Option<u64>| -> Result<u64> {
            given
                .or(under)
                .with_context(|| format!("Link's specification sets no {key}"))
        };
        Ok(Opinions {
            memory: size("memory", &tool.memory, under.map(|u| u.memory))?,
            reserve: size("reserve", &tool.reserve, under.map(|u| u.reserve))?,
            file: size("file", &tool.file, under.map(|u| u.file))?,
            threads: number(
                "threads",
                tool.threads.map(u64::from),
                under.map(|u| u.threads as u64),
            )? as usize,
            deadline: match (&tool.deadline, under) {
                (Some(deadline), _) => {
                    std::time::Duration::from_millis(link_agent_spec::millis(deadline))
                }
                (None, Some(under)) => under.deadline,
                (None, None) => anyhow::bail!("Link's specification sets no deadline"),
            },
            dimensions: number(
                "dimensions",
                tool.dimensions.map(u64::from),
                under.map(|u| u64::from(u.dimensions)),
            )? as u32,
            folders,
            kept_off: size("kept_off", &tool.kept_off, under.map(|u| u.kept_off))?,
        })
    }

    /// What a `serve` line says, each flag left out Link's
    /// specification's.
    pub fn served(serve: &MemoryToolServeArgs) -> Result<Opinions> {
        let link = Opinions::link();
        Ok(Opinions {
            memory: serve.index_memory.unwrap_or(link.memory),
            reserve: serve.reserve.unwrap_or(link.reserve),
            file: serve.file_most.unwrap_or(link.file),
            threads: serve.threads.map_or(link.threads, |n| n as usize),
            deadline: serve
                .deadline_ms
                .map_or(link.deadline, std::time::Duration::from_millis),
            dimensions: serve.dimensions.map_or(link.dimensions, u32::from),
            folders: match serve.folders.is_empty() {
                false => serve.folders.clone(),
                true => link.folders.clone(),
            },
            kept_off: serve.kept_off.unwrap_or(link.kept_off),
        })
    }

    /// Writes its values into `serve`'s flags.
    pub fn serve_with(&self, serve: &mut MemoryToolServeArgs) {
        serve.index_memory = Some(self.memory);
        serve.reserve = Some(self.reserve);
        serve.file_most = Some(self.file);
        serve.threads = u16::try_from(self.threads).ok();
        serve.deadline_ms = u64::try_from(self.deadline.as_millis()).ok();
        serve.dimensions = u16::try_from(self.dimensions).ok();
        serve.folders = self.folders.clone();
        serve.kept_off = Some(self.kept_off);
    }

    /// Whether it indexes `folder`.
    pub fn indexes(&self, folder: Indexed) -> bool {
        self.folders.contains(&folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Link's specification sets every key, and each reads as it says.
    #[test]
    fn link_sets_every_opinion() {
        let link = Opinions::link();
        assert_eq!(link.memory, 512 << 20);
        assert_eq!(link.reserve, 1 << 30);
        assert_eq!(link.file, 16 << 20);
        assert_eq!(link.threads, 1);
        assert_eq!(link.deadline, std::time::Duration::from_millis(300));
        assert_eq!(link.dimensions, 256);
        assert_eq!(link.folders, [Indexed::Memory, Indexed::Chats]);
        assert_eq!(link.kept_off, 2 << 30);
    }

    /// Each key a specification's `[tool.memory]` sets reaches the
    /// service: written as its `serve` flag, read back from the line the
    /// service reads; a key left out is Link's specification's.
    #[test]
    fn each_key_reaches_the_service_by_its_flag() {
        let tool: link_agent_spec::Tool = serde_json::from_value(serde_json::json!({
            "memory": "64 MB", "reserve": "2 GB", "file": "4 MB", "threads": 3,
            "deadline": "150ms", "dimensions": 512, "folders": ["memory"],
            "kept_off": "8 MB",
        }))
        .unwrap();
        let chosen = Opinions::of(&tool, Some(Opinions::link())).unwrap();
        assert_eq!(
            chosen,
            Opinions {
                memory: 64 << 20,
                reserve: 2 << 30,
                file: 4 << 20,
                threads: 3,
                deadline: std::time::Duration::from_millis(150),
                dimensions: 512,
                folders: vec![Indexed::Memory],
                kept_off: 8 << 20,
            }
        );
        let mut serve = crate::cli::serve_line(
            &["lnk-memory-tool".to_string(), "serve".into(), "/f".into()],
            &|_| None,
        )
        .unwrap();
        // Nothing said: Link's specification's.
        assert_eq!(&Opinions::served(&serve).unwrap(), Opinions::link());
        chosen.serve_with(&mut serve);
        let line: Vec<String> = std::iter::once("lnk-memory-tool".to_string())
            .chain(crate::cli::serve_argv(&serve))
            .collect();
        let read = crate::cli::serve_line(&line, &|_| None).unwrap();
        assert_eq!(Opinions::served(&read).unwrap(), chosen);
        // One key alone: the rest Link's.
        let one: link_agent_spec::Tool =
            serde_json::from_value(serde_json::json!({"reserve": "10 GB"})).unwrap();
        let one = Opinions::of(&one, Some(Opinions::link())).unwrap();
        assert_eq!(one.reserve, 10 << 30);
        assert_eq!(one.file, Opinions::link().file);
    }
}
