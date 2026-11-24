//! What a caller and a driver say: the limits, the [`Contract`], each
//! [`Request`] and its [`Reply`], and the checks of what either sends.

use super::*;

/// The folder a driver on this machine keeps what it keeps in, in its
/// environment: the agent's `chats/` in its own folder
/// ([`crate::folder::Folder::of`]); for `lnk-file-store`, its own folder.
pub const DIR_ENV: &str = "LNK_STORE_DIR";

/// The agent whose things a driver keeping one agent's in a folder
/// keeps there, in its environment (`<id>`): it refuses any other's.
pub const AGENT_ENV: &str = "LNK_STORE_AGENT";

/// The version of this contract.
pub const VERSION: u32 = 1;

/// The most an event is, as JSON: a message is at most 2 MiB (a connection's
/// endpoint's limit), and an answer less.
pub const EVENT_MAX: u64 = 16 << 20;

/// The longest a call may take before its driver is stopped.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// What `lnk-<name> contract` prints.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contract {
    /// The version of this contract it speaks ([`VERSION`]).
    pub version: u32,
    /// Whether it answers `lnk-<name> serve`, calls one a line for as
    /// long as its caller keeps it running.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub serve: bool,
    /// Whether it keeps agents' jobs ([`Request::JobPut`] and the rest).
    /// A driver that leaves it out is never asked them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub jobs: bool,
    /// Whether it keeps agents' files by path ([`Request::FileRead`] and
    /// the rest). A driver that leaves it out is never asked them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub files: bool,
}

impl Contract {
    /// This version's, serving, keeping conversations alone: a driver
    /// that keeps more says so (`Contract { jobs: true, ..Contract::serving() }`).
    pub fn serving() -> Self {
        Self {
            version: VERSION,
            serve: true,
            jobs: false,
            files: false,
        }
    }
}

/// The most a job's spec is, as JSON.
pub const JOB_MAX: u64 = 1 << 20;

/// The longest a job's ID is, in bytes.
pub const JOB_ID_MAX: usize = 256;

/// Where a job is: waiting for its time, being fired, its firing
/// ending (taken from being fired, so a cancel meanwhile finds it in
/// neither), or ended, kept with its outcome for whoever waits on it.
/// An ended job is never due.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    Waiting,
    Firing,
    Ending,
    Ended,
}

/// A job, as a driver keeps it: its ID, its spec (the scheduler's, JSON,
/// none of it the driver's to read) and when it's due, in seconds since
/// 1970.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub spec: Value,
    pub due: u64,
}

/// The most a file of an agent's collections is, as a driver keeps one:
/// an instruction or a skill's.
pub const FILE_MAX: u64 = 4 << 20;

/// A file a driver keeps: its bytes, as base64, and its version, which a
/// write names (none written yet: 0).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub data: String,
    pub version: u64,
}

/// The most a file of memory's is, as a driver keeps one: a segment of
/// its index's copy ([`crate::files::index::SEGMENT`], 8 MiB), and room.
pub const MEMORY_FILE_MAX: u64 = 16 << 20;

/// The most file `path` is: [`MEMORY_FILE_MAX`] in `memory`, else
/// [`FILE_MAX`].
pub fn file_max(path: &str) -> u64 {
    match crate::storage::Collection::of_path(path) {
        Some(crate::storage::Collection::Memory) => MEMORY_FILE_MAX,
        _ => FILE_MAX,
    }
}

/// The most paths a [`Request::FileList`] answers: a folder of more is
/// refused rather than cut short.
pub const FILES_MAX: usize = 4096;

/// The collections whose files a driver keeps by path
/// ([`Request::FileRead`] and the rest): an agent's instructions, its
/// skills, and memory's (its index's copy). Its conversations and jobs
/// have calls of their own.
pub const FILE_COLLECTIONS: [crate::storage::Collection; 3] = [
    crate::storage::Collection::Instructions,
    crate::storage::Collection::Skills,
    crate::storage::Collection::Memory,
];

/// An error unless `path` is a path of the agent's instructions or skills
/// ([`FILE_COLLECTIONS`]): relative, its parts neither empty, `.` nor
/// `..`, at most 1,024 bytes. `folder` allows a folder (`skills/`).
pub fn check_file_path(path: &str, folder: bool) -> anyhow::Result<()> {
    let trimmed = match folder {
        true => path.strip_suffix('/').unwrap_or(path),
        false => path,
    };
    anyhow::ensure!(
        !trimmed.is_empty()
            && trimmed.len() <= 1024
            && !trimmed.starts_with('/')
            && !trimmed.contains('\0')
            && !trimmed.contains('\\')
            && trimmed
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
            && crate::storage::Collection::of_path(trimmed)
                .is_some_and(|c| FILE_COLLECTIONS.contains(&c)),
        "{:?} isn't a path of the agent's instructions, skills or memory",
        link_format::clean(path)
    );
    Ok(())
}

/// An error for file `path`, or what's added to it, `len` bytes, larger
/// than its most ([`file_max`]).
pub fn check_file_size(path: &str, len: usize) -> anyhow::Result<()> {
    let max = file_max(path);
    anyhow::ensure!(
        len as u64 <= max,
        "{} is at most {max} bytes, not {len}",
        link_format::clean(path)
    );
    Ok(())
}

/// The bytes of file `path` a request carries, as base64: an error for
/// what isn't, or is larger than its most ([`file_max`]).
pub fn file_data(path: &str, data: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine as _;
    let max = file_max(path);
    // Base64 is 4 bytes for every 3: a larger one is refused unread.
    anyhow::ensure!(
        data.len() as u64 <= max.div_ceil(3) * 4,
        "{} is at most {max} bytes",
        link_format::clean(path)
    );
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .context("a file's data that isn't base64")?;
    check_file_size(path, bytes.len())?;
    Ok(bytes)
}

/// An error unless `tools` is a list a conversation may be narrowed to:
/// at most [`crate::connection::TOOLS_MAX`], each a tool's name
/// ([`crate::connection::is_tool_name`]).
pub fn check_tools(tools: &[String]) -> anyhow::Result<()> {
    crate::connection::tools_header(tools).map(|_| ())
}

/// An error unless `id` is a job's ID: 1 to [`JOB_ID_MAX`] bytes, no
/// control character.
pub fn check_job_id(id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        (1..=JOB_ID_MAX).contains(&id.len()) && !id.chars().any(char::is_control),
        "{:?} isn't a job's ID",
        link_format::clean(id)
    );
    Ok(())
}

/// An error unless `job` is one a driver keeps: its ID one, its spec at
/// most [`JOB_MAX`].
pub fn check_job(job: &Job) -> anyhow::Result<()> {
    check_job_id(&job.id)?;
    anyhow::ensure!(
        serde_json::to_vec(&job.spec)?.len() as u64 <= JOB_MAX,
        "job {}'s spec is larger than {JOB_MAX} bytes",
        link_format::clean(&job.id)
    );
    Ok(())
}

/// Whose conversations: an agent's ID and its owner, as Link names them
/// (`LNK_AGENT_ID`, `LNK_OWNER`). A driver on this machine may key by
/// [`DIR_ENV`] alone, which is the agent's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub owner: String,
}

/// One call of a driver.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Request {
    /// Writes `event` as number `event.seq`, if it's free and the next:
    /// [`Reply::Done`], else [`Reply::Taken`].
    Append {
        agent: Agent,
        thread: String,
        event: Value,
    },
    /// The events numbered `first` and on, in order: [`Reply::Events`],
    /// none for a conversation it doesn't keep.
    Read {
        agent: Agent,
        thread: String,
        first: u64,
    },
    /// The last event's number, 0 for none: [`Reply::Last`].
    Last { agent: Agent, thread: String },
    /// Every conversation it keeps of `agent`, with its last number:
    /// [`Reply::Threads`].
    Threads { agent: Agent },
    /// Forgets a conversation, its events and its lease: [`Reply::Done`].
    Remove { agent: Agent, thread: String },
    /// Takes the lease for `holder`: [`Reply::Took`].
    Take {
        agent: Agent,
        thread: String,
        holder: String,
        /// How long a lease not renewed lasts, in milliseconds.
        lapse_ms: u64,
    },
    /// Renews `holder`'s lease: [`Reply::Done`], or [`Reply::Lost`] when
    /// it isn't `holder`'s.
    Renew {
        agent: Agent,
        thread: String,
        holder: String,
    },
    /// Lets `holder`'s lease go; nothing when it isn't `holder`'s:
    /// [`Reply::Done`].
    End {
        agent: Agent,
        thread: String,
        holder: String,
    },
    /// Every lease of `agent`'s conversations, each judged lapsed or not
    /// by `lapse_ms`: [`Reply::Leases`].
    Leases { agent: Agent, lapse_ms: u64 },
    /// Keeps `job` in `place`, in place of any there with its ID:
    /// [`Reply::Done`]. A driver whose contract says `jobs` only, as all
    /// below.
    #[serde(rename = "job_put")]
    JobPut {
        agent: Agent,
        place: Place,
        job: Job,
    },
    /// Job `id` in `place`, if it's there: [`Reply::Job`].
    #[serde(rename = "job_get")]
    JobGet {
        agent: Agent,
        place: Place,
        id: String,
    },
    /// Every job in `place`, in no order: [`Reply::Jobs`].
    #[serde(rename = "job_list")]
    JobList { agent: Agent, place: Place },
    /// How many jobs are in `place`, none read: [`Reply::Count`].
    #[serde(rename = "job_count")]
    JobCount { agent: Agent, place: Place },
    /// Moves job `id` from `from` to `to`, in place of any there with its
    /// ID, whole: [`Reply::Found`], whether it was in `from`. Of two
    /// shifts of one job from one place, one is made.
    #[serde(rename = "job_shift")]
    JobShift {
        agent: Agent,
        id: String,
        from: Place,
        to: Place,
    },
    /// Forgets job `id` in `place`: [`Reply::Found`], whether it was
    /// there.
    #[serde(rename = "job_remove")]
    JobRemove {
        agent: Agent,
        place: Place,
        id: String,
    },
    /// When the soonest job waiting is due, none if none waits:
    /// [`Reply::Next`].
    #[serde(rename = "job_next")]
    JobNext { agent: Agent },
    /// The day's count of the agent's messages: [`Reply::Day`], `(0, 0)`
    /// for none kept.
    #[serde(rename = "job_day")]
    JobDay { agent: Agent },
    /// Keeps the day's count: [`Reply::Done`].
    #[serde(rename = "job_set_day")]
    JobSetDay { agent: Agent, day: u64, sent: u32 },
    /// A mark that differs once a job is put in, shifted to or from, or
    /// removed from waiting: [`Reply::Mark`].
    #[serde(rename = "job_mark")]
    JobMark { agent: Agent },
    /// The agent's file at `path`, a path of its instructions or skills
    /// (`instructions/instruction.md`, [`check_file_path`]):
    /// [`Reply::File`], none when there's none.
    #[serde(rename = "file_read")]
    FileRead { agent: Agent, path: String },
    /// The paths of the agent's files under `folder` (`skills/`), a
    /// folder of its instructions or skills, in order, at most
    /// [`FILES_MAX`]: [`Reply::Files`].
    #[serde(rename = "file_list")]
    FileList { agent: Agent, folder: String },
    /// Writes the agent's file at `path` with `data` (base64, at most
    /// [`FILE_MAX`] bytes) if its version is still `version` (0: there's
    /// none): [`Reply::Wrote`], its new version, else [`Reply::Stale`],
    /// its version now. Of two writers of one version, one writes.
    #[serde(rename = "file_write")]
    FileWrite {
        agent: Agent,
        path: String,
        data: String,
        version: u64,
    },
    /// Removes the agent's file at `path` if its version is still
    /// `version`: [`Reply::Found`], whether it did.
    #[serde(rename = "file_remove")]
    FileRemove {
        agent: Agent,
        path: String,
        version: u64,
    },
    /// Adds `data` (base64) at the end of the agent's file at `path`, made
    /// if there's none, in one step: of two appends at once, both are
    /// kept, one after the other, never one within the other.
    /// [`Reply::Wrote`], its new version. A file that would grow past
    /// [`FILE_MAX`] is refused, left as it was.
    #[serde(rename = "file_append")]
    FileAppend {
        agent: Agent,
        path: String,
        data: String,
    },
    /// The tools conversation `thread` was narrowed to: [`Reply::Tools`],
    /// none when nothing narrowed it, and it has every tool of its
    /// agent's.
    #[serde(rename = "tools_read")]
    ToolsRead { agent: Agent, thread: String },
    /// Every conversation of the agent's a list narrowed, with what it
    /// may use: [`Reply::Narrowed`]. One removed is among them, its
    /// narrowing kept ([`Request::Remove`]), as is one narrowed before
    /// its first event: [`Request::Threads`] lists neither, so a move
    /// carries each from here.
    #[serde(rename = "tools_list")]
    ToolsList { agent: Agent },
    /// Narrows conversation `thread` to `tools`, of those it may use, in
    /// one step: a tool it may not use stays out, so a list never widens
    /// it. [`Reply::Tools`], what it may use now.
    #[serde(rename = "tools_narrow")]
    ToolsNarrow {
        agent: Agent,
        thread: String,
        tools: Vec<String>,
    },
}

impl Request {
    /// Whose conversations it's for.
    pub fn agent(&self) -> &Agent {
        match self {
            Self::Append { agent, .. }
            | Self::Read { agent, .. }
            | Self::Last { agent, .. }
            | Self::Threads { agent }
            | Self::Remove { agent, .. }
            | Self::Take { agent, .. }
            | Self::Renew { agent, .. }
            | Self::End { agent, .. }
            | Self::Leases { agent, .. }
            | Self::JobPut { agent, .. }
            | Self::JobGet { agent, .. }
            | Self::JobList { agent, .. }
            | Self::JobCount { agent, .. }
            | Self::JobShift { agent, .. }
            | Self::JobRemove { agent, .. }
            | Self::JobNext { agent }
            | Self::JobDay { agent }
            | Self::JobSetDay { agent, .. }
            | Self::JobMark { agent }
            | Self::FileRead { agent, .. }
            | Self::FileList { agent, .. }
            | Self::FileWrite { agent, .. }
            | Self::FileRemove { agent, .. }
            | Self::FileAppend { agent, .. }
            | Self::ToolsRead { agent, .. }
            | Self::ToolsList { agent }
            | Self::ToolsNarrow { agent, .. } => agent,
        }
    }

    /// The collection it's of: a conversation's and its tools'
    /// `conversations`, a job's `schedule`, a file's the collection its
    /// path's first part names. None for a path in none of them.
    pub fn collection(&self) -> Option<crate::storage::Collection> {
        use crate::storage::Collection;
        match self {
            Self::FileRead { path, .. }
            | Self::FileWrite { path, .. }
            | Self::FileRemove { path, .. }
            | Self::FileAppend { path, .. } => Collection::of_path(path),
            Self::FileList { folder, .. } => Collection::of_path(folder),
            _ if self.of_jobs() => Some(Collection::Schedule),
            _ => Some(Collection::Conversations),
        }
    }

    /// The path of the file it changes (writes, adds to or removes), if
    /// it changes one.
    pub fn changed_file(&self) -> Option<&str> {
        match self {
            Self::FileWrite { path, .. }
            | Self::FileRemove { path, .. }
            | Self::FileAppend { path, .. } => Some(path),
            _ => None,
        }
    }

    /// Whether it's of an agent's files by path, which only a driver
    /// whose contract says `files` is asked.
    pub fn of_files(&self) -> bool {
        matches!(
            self,
            Self::FileRead { .. }
                | Self::FileList { .. }
                | Self::FileWrite { .. }
                | Self::FileRemove { .. }
                | Self::FileAppend { .. }
        )
    }

    /// Whether it's of an agent's jobs, which only a driver whose
    /// contract says `jobs` is asked.
    pub fn of_jobs(&self) -> bool {
        matches!(
            self,
            Self::JobPut { .. }
                | Self::JobGet { .. }
                | Self::JobList { .. }
                | Self::JobCount { .. }
                | Self::JobShift { .. }
                | Self::JobRemove { .. }
                | Self::JobNext { .. }
                | Self::JobDay { .. }
                | Self::JobSetDay { .. }
                | Self::JobMark { .. }
        )
    }
}

/// A driver's answer to a [`Request`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "lowercase")]
pub enum Reply {
    Done,
    /// The number wasn't free, or wasn't the next: the conversation's
    /// last number is `last`.
    Taken {
        last: u64,
    },
    Events {
        events: Vec<Value>,
    },
    Last {
        last: u64,
    },
    Threads {
        threads: Vec<Kept>,
    },
    Took {
        took: Took,
    },
    /// The lease isn't the holder's any more: it lapsed, and another took
    /// it.
    Lost,
    Leases {
        leases: Vec<Lease>,
    },
    /// A job, or none: left out.
    Job {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job: Option<Job>,
    },
    Jobs {
        jobs: Vec<Job>,
    },
    Count {
        count: u64,
    },
    /// Whether the job was where the call said.
    Found {
        found: bool,
    },
    /// When the soonest job waiting is due.
    Next {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next: Option<u64>,
    },
    /// The day's count: the day, as days since 1970 in the agent's time
    /// zone, and how many messages went on it.
    Day {
        day: u64,
        sent: u32,
    },
    Mark {
        mark: u64,
    },
    /// A file, its bytes as base64 and its version; none when there's
    /// none.
    File {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<Stored>,
    },
    /// The paths of files.
    Files {
        files: Vec<String>,
    },
    /// Written: the file's version now.
    Wrote {
        version: u64,
    },
    /// Not written, its version not the one named: its version now, 0
    /// for none.
    Stale {
        version: u64,
    },
    /// The tools a conversation may use: none for every tool.
    Tools {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tools: Option<Vec<String>>,
    },
    /// Each conversation narrowed, by its thread, and what it may use.
    Narrowed {
        narrowed: BTreeMap<String, Vec<String>>,
    },
    /// The call failed, and why.
    Error {
        error: String,
    },
}

/// A conversation a driver keeps.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Kept {
    pub thread: String,
    /// Its last event's number.
    pub last: u64,
}

/// What taking a lease came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Took {
    /// Taken: no turn ran.
    Took,
    /// Taken from a holder whose lease lapsed: its turn crashed.
    Lapsed,
    /// Another's, renewed lately.
    Held,
}

/// A conversation's lease: who holds it, when it was taken and last
/// renewed (seconds since 1970, the driver's clock), and whether it
/// lapsed, as [`Request::Leases`] judged it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub thread: String,
    pub holder: String,
    pub started: u64,
    pub renewed: u64,
    #[serde(default)]
    pub lapsed: bool,
}

/// What a write of a file came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    /// Written: its new version.
    Version(u64),
    /// Its version wasn't the one named: its version now, 0 for none.
    Stale(u64),
}

/// What an append came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appended {
    Written,
    /// Not free, or not the next: the last number written.
    Taken(u64),
}

/// An event's number: its `seq`.
pub fn seq_of(event: &Value) -> Option<u64> {
    event.get("seq")?.as_u64()
}

/// A store driver's name, as `[storage] conversations` names it and its
/// program is `lnk-<name>`: lowercase letters, digits and `-`, starting
/// with a letter, at most 64; never `log` or `compact`, which aren't
/// drivers.
pub fn check_name(name: &str) -> anyhow::Result<()> {
    let plain = (1..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    anyhow::ensure!(
        plain,
        "{:?} isn't a store driver's name",
        link_format::clean(name)
    );
    anyhow::ensure!(
        ![crate::storage::LOG, crate::storage::COMPACT].contains(&name),
        "{name} isn't a driver: Link Harness keeps it itself"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_replies_read_back() {
        let request = Request::Take {
            agent: Agent {
                id: "a-1".into(),
                owner: "me@box".into(),
            },
            thread: "telegram:42".into(),
            holder: "h".into(),
            lapse_ms: 60_000,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.starts_with(r#"{"op":"take","#), "{json}");
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), request);
        let reply = Reply::Took { took: Took::Lapsed };
        let json = serde_json::to_string(&reply).unwrap();
        assert_eq!(json, r#"{"reply":"took","took":"lapsed"}"#);
        assert_eq!(
            serde_json::to_string(&Reply::Done).unwrap(),
            r#"{"reply":"done"}"#
        );
    }

    /// Every reply, and the contract, is its declaration's shape: written
    /// as its declared type, it's the same JSON, an absent value left out.
    #[test]
    fn every_reply_is_its_declared_shape() {
        let job = Job {
            id: "j1".into(),
            spec: serde_json::json!({"text": "hi"}),
            due: 7,
        };
        let replies = [
            Reply::Done,
            Reply::Taken { last: 3 },
            Reply::Events {
                events: vec![serde_json::json!({"seq": 1})],
            },
            Reply::Last { last: 0 },
            Reply::Threads {
                threads: vec![Kept {
                    thread: "t".into(),
                    last: 2,
                }],
            },
            Reply::Took { took: Took::Held },
            Reply::Lost,
            Reply::Leases {
                leases: vec![Lease {
                    thread: "t".into(),
                    holder: "h".into(),
                    started: 1,
                    renewed: 2,
                    lapsed: true,
                }],
            },
            Reply::Job {
                job: Some(job.clone()),
            },
            Reply::Job { job: None },
            Reply::Jobs { jobs: vec![job] },
            Reply::Count { count: 4 },
            Reply::Found { found: false },
            Reply::Next { next: Some(9) },
            Reply::Next { next: None },
            Reply::Day {
                day: 20000,
                sent: 5,
            },
            Reply::Mark { mark: 6 },
            Reply::File {
                file: Some(Stored {
                    data: "aGk=".into(),
                    version: 1,
                }),
            },
            Reply::File { file: None },
            Reply::Files {
                files: vec!["skills/a.md".into()],
            },
            Reply::Wrote { version: 2 },
            Reply::Stale { version: 0 },
            Reply::Tools {
                tools: Some(vec!["memory__search".into()]),
            },
            Reply::Tools { tools: None },
            Reply::Narrowed {
                narrowed: BTreeMap::from([("t".into(), vec!["a".into()])]),
            },
            Reply::Error {
                error: "no such".into(),
            },
        ];
        // The same members, in the declaration's order.
        let read = |line: Vec<u8>| serde_json::from_slice::<Value>(&line).unwrap();
        for reply in replies {
            let written = serde_json::to_value(&reply).unwrap();
            let declared =
                link_actions::answer::json_line::<crate::declared::Reply>(&reply).unwrap();
            assert_eq!(read(declared), written, "{reply:?}");
            assert!(!written.to_string().contains("null"));
        }
        let contract = Contract {
            jobs: true,
            ..Contract::serving()
        };
        assert_eq!(
            read(link_actions::answer::json_line::<crate::declared::Contract>(&contract).unwrap()),
            serde_json::to_value(&contract).unwrap()
        );
    }

    /// A path of the agent's files is of its instructions or its skills
    /// alone, the collections a driver keeps by path, and is refused
    /// saying so.
    #[test]
    fn a_files_path_is_of_its_instructions_skills_or_memory() {
        for good in [
            "instructions/base.md",
            "skills/greet/SKILL.md",
            "memory/link-memory/manifest.json",
        ] {
            assert!(check_file_path(good, false).is_ok(), "{good}");
        }
        for good in ["skills/", "skills", "instructions/"] {
            assert!(check_file_path(good, true).is_ok(), "{good}");
        }
        for bad in [
            "chats/telegram-42/events.jsonl",
            "schedule/jobs.json",
            "work/notes.md",
            "skills/../memory/x",
            "/instructions/base.md",
            "skills//x",
            "skills/",
            "",
        ] {
            let err = check_file_path(bad, false).unwrap_err().to_string();
            assert!(
                err.ends_with("isn't a path of the agent's instructions, skills or memory"),
                "{bad:?}: {err}"
            );
        }
    }

    /// A driver's contract says whether it serves; one that leaves it
    /// out doesn't.
    #[test]
    fn a_contract_says_whether_it_serves() {
        let old: Contract = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert!(!old.serve);
        let serving = Contract::serving();
        assert_eq!(
            serde_json::to_string(&serving).unwrap(),
            r#"{"version":1,"serve":true}"#
        );
        assert_eq!(serde_json::to_string(&old).unwrap(), r#"{"version":1}"#);
        // Jobs said only by a driver that keeps them.
        assert!(!old.jobs);
        let keeping = Contract {
            jobs: true,
            ..Contract::serving()
        };
        assert_eq!(
            serde_json::to_string(&keeping).unwrap(),
            r#"{"version":1,"serve":true,"jobs":true}"#
        );
        // Files the same.
        assert!(!old.files);
        let filing = Contract {
            files: true,
            ..Contract::serving()
        };
        assert_eq!(
            serde_json::to_string(&filing).unwrap(),
            r#"{"version":1,"serve":true,"files":true}"#
        );
    }

    /// The jobs' requests and replies as they go on the wire: each named
    /// `job_<op>`, each place a word.
    #[test]
    fn jobs_requests_and_replies_read_back() {
        let agent = Agent {
            id: "a-1".into(),
            owner: "me@box".into(),
        };
        let shift = Request::JobShift {
            agent: agent.clone(),
            id: "j1".into(),
            from: Place::Waiting,
            to: Place::Firing,
        };
        let json = serde_json::to_string(&shift).unwrap();
        assert!(json.starts_with(r#"{"op":"job_shift","#), "{json}");
        assert!(json.contains(r#""from":"waiting","to":"firing""#), "{json}");
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), shift);
        assert!(shift.of_jobs());
        assert!(!Request::Threads { agent }.of_jobs());
        let reply = Reply::Job {
            job: Some(Job {
                id: "j1".into(),
                spec: serde_json::json!({"text": "hi"}),
                due: 7,
            }),
        };
        let json = serde_json::to_string(&reply).unwrap();
        assert_eq!(
            json,
            r#"{"reply":"job","job":{"id":"j1","spec":{"text":"hi"},"due":7}}"#
        );
        assert_eq!(serde_json::from_str::<Reply>(&json).unwrap(), reply);
        assert_eq!(
            serde_json::to_string(&Reply::Next { next: None }).unwrap(),
            r#"{"reply":"next"}"#
        );
        for bad in ["", "a\nb", &"x".repeat(JOB_ID_MAX + 1)] {
            assert!(check_job_id(bad).is_err(), "{bad:?}");
        }
    }
}
