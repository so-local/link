//! A conversation's idle compaction (`[window] compact_after`), as its
//! scheduler keeps it: one job a conversation, its ID made from the
//! conversation's name, so the harness keeping it moves the one job
//! with each message, and knows its firing when it comes.

use std::sync::LazyLock;

use serde::Deserialize;

/// What `compaction.toml` names.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compaction {
    /// The tool the scheduler is among an agent's: its server's name.
    pub tool: String,
    /// The scheduler's MCP call that keeps a conversation's job.
    pub call: String,
    /// The call's argument: how long from now, in seconds, it's due.
    pub after: String,
    /// The job's message, read only in its logs.
    pub text: String,
}

/// `compaction.toml`, read once.
pub fn compaction() -> &'static Compaction {
    static READ: LazyLock<Compaction> = LazyLock::new(|| {
        toml::from_str(include_str!("compaction.toml")).expect("compaction.toml is a Compaction")
    });
    &READ
}

/// The ID of `thread`'s compaction job: an ID's shape (`link_id`), made
/// from the conversation's name, the same every time, so keeping it
/// again replaces the one waiting.
pub fn job(thread: &str) -> String {
    let limits = link_id::limits();
    let alphabet = limits.candidate_alphabet.as_bytes();
    // FNV-1a, 64 bits: stable across builds and machines, as a hash of
    // the standard library's isn't.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut id = String::with_capacity(limits.candidate_length);
    for round in 0..limits.candidate_length {
        for byte in thread.bytes().chain([round as u8]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        id.push(char::from(
            alphabet[(hash % alphabet.len() as u64) as usize],
        ));
    }
    id
}

/// Who `thread`'s compaction job's message is from, as its firing says:
/// a schedule's sender, `schedule:<id>`.
pub fn from(thread: &str) -> String {
    format!("schedule:{}", job(thread))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A conversation's job ID is an ID, the same each time, and
    /// another conversation's differs.
    #[test]
    fn a_conversations_job_is_named_by_it() {
        let one = job("telegram:42");
        assert!(link_id::is_candidate(&one), "{one}");
        assert_eq!(one, job("telegram:42"));
        assert_ne!(one, job("telegram:43"));
        assert_eq!(from("telegram:42"), format!("schedule:{one}"));
    }

    /// The names it reads are set.
    #[test]
    fn its_names_are_read() {
        let read = compaction();
        assert_eq!(read.tool, "cron");
        assert_eq!(read.call, "compact");
        assert_eq!(read.after, "after");
        assert!(!read.text.is_empty());
    }

    /// Its call is one the tool declares, taking its argument, so the
    /// namespace forwarding it checks it as any other call; and it never
    /// asks, even of a spec that asks first.
    #[test]
    fn its_call_is_declared_and_never_asks() {
        let read = compaction();
        let template = link_actions::tools::Template::parse(crate::TOOLS).unwrap();
        let tool = template
            .tools
            .iter()
            .find(|tool| tool.name == read.call)
            .expect("compact declared");
        assert!(tool.input_schema["properties"].get(&read.after).is_some());
        let asking = crate::agent_spec::Tool {
            ask: Some(true),
            ..Default::default()
        };
        assert!(crate::allowed(&asking).unwrap().contains(&read.call));
    }
}
