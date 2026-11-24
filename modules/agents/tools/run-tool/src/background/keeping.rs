//! A command's record, its lock and its stop asked, where they are kept,
//! and what tells its process group is its own.

use super::*;

/// How a background command is, or ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Running, under its supervisor.
    Running,
    /// It exited, with `exit`.
    Exited,
    /// A signal ended it, `signal`.
    Signalled,
    /// It ran past its timeout and was stopped.
    #[serde(rename = "timed out")]
    TimedOut,
    /// The agent stopped it.
    Stopped,
    /// Its supervisor ended without it: the agent stopped or restarted,
    /// or the machine did.
    Lost,
}

impl State {
    pub(super) fn running(self) -> bool {
        self == State::Running
    }
}

/// A background command's record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    /// The command as the agent gave it.
    pub command: String,
    /// What runs: the sandbox, then the shell, `-c` and the command.
    pub argv: Vec<String>,
    /// The PATH it runs with.
    pub path: String,
    /// The files folder it starts in and its output is in.
    pub files: PathBuf,
    /// The conversation that started it, told when it ends.
    pub thread: String,
    /// The hops of the call that started it.
    pub hops: u32,
    /// Its bridge's way in.
    pub way_in: PathBuf,
    /// The agent its ending is told as. None: told as nobody, so never
    /// taken.
    #[serde(default)]
    pub caller: Caller,
    /// The longest it runs, in milliseconds.
    pub timeout_ms: u64,
    /// What's kept on disk of its output.
    pub output_max: u64,
    /// How much of its output's end its ending says.
    pub tail: u64,
    /// How long it's kept once it ended and was told, in seconds.
    pub keep_secs: u64,
    /// When it started and ended, in seconds since 1970.
    pub started: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<u64>,
    /// How long it ran, in milliseconds, once it ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    /// Why it was lost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Its process group, its leader's start time and the machine's boot
    /// it ran in: what a sweep checks before it kills what's left of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_start: Option<u64>,
    #[serde(default)]
    pub boot: String,
    /// The bytes of its output dropped from its start.
    #[serde(default)]
    pub dropped: u64,
    /// The output file `dropped` counts for (its inode): one cut since
    /// is another file, read once the record catches up.
    #[serde(default)]
    pub output_file: u64,
    /// Whether the agent was told how it ended.
    #[serde(default)]
    pub told: bool,
}

link_files::specification!(Record, include_str!("record.schema.toml"));

impl Record {
    /// Its command, as one line.
    pub fn line(&self) -> String {
        self.command
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// How it is or ended, in words.
    pub fn how(&self) -> String {
        match self.state {
            State::Running => "running".into(),
            State::Exited => format!("exit {}", self.exit.unwrap_or(-1)),
            State::Signalled => format!("ended by signal {}", self.signal.unwrap_or(0)),
            State::TimedOut => format!(
                "stopped at its timeout, {}",
                took(Duration::from_millis(self.timeout_ms))
            ),
            State::Stopped => "stopped".into(),
            State::Lost => match &self.why {
                Some(why) => format!("lost: {why}"),
                None => "lost".into(),
            },
        }
    }

    /// How long it ran, or has run.
    pub(super) fn ran(&self) -> Duration {
        match self.took_ms {
            Some(ms) => Duration::from_millis(ms),
            None => Duration::from_secs(now().saturating_sub(self.started)),
        }
    }

    /// Its output's size, all of it, dropped bytes included.
    pub fn bytes(&self) -> u64 {
        self.dropped + output_size(&self.files, &self.id).unwrap_or(0)
    }

    /// It, as `list` gives it.
    pub fn value(&self) -> Value {
        json!({
            "id": self.id,
            "command": self.line(),
            "state": self.state,
            "exit": self.exit,
            "signal": self.signal,
            "seconds": self.ran().as_secs(),
            "output_bytes": self.bytes(),
            "output": output_name(&self.id),
        })
    }

    /// Its ending, as the agent is told it.
    pub fn ending(&self) -> String {
        let mut text = format!(
            "Your background command {} ended: {}, after {}.\n{}",
            self.id,
            self.how(),
            took(self.ran()),
            self.line()
        );
        let total = self.bytes();
        let end = read_output(self, total.saturating_sub(self.tail), self.tail).ok();
        match end {
            // Its control characters (all but newlines and tabs) dropped:
            // the turn this starts reads it as text, never a terminal's
            // escapes. Fenced as a program's output, as `task__read`
            // fences it: it starts a turn, so it's never read as the
            // turn's instructions.
            Some(read) if !read.text.is_empty() => {
                let plain = read
                    .text
                    .chars()
                    .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
                    .collect::<String>();
                let source = link_untrusted::Source {
                    kind: link_untrusted::Kind::Output,
                    name: &self.id,
                };
                let said = link_untrusted::fence(source, &plain)
                    .unwrap_or_else(|_| "(withheld: no randomness to fence it)".to_string());
                text.push_str(&format!(
                    "\n\nIts output's end ({} of {total} bytes):\n{said}",
                    read.to - read.from,
                ))
            }
            _ => text.push_str("\n\nIt printed nothing."),
        }
        text.push_str(&format!(
            "\n\nAll of it: task__read, or {}.",
            output_name(&self.id)
        ));
        text
    }
}

/// `duration` as a person says it: `42s`, `3m 12s`, `1h 5m`.
pub(super) fn took(duration: Duration) -> String {
    let secs = duration.as_secs();
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m {}s", secs / 60, secs % 60),
        _ => format!("{}h {}m", secs / 3600, secs % 3600 / 60),
    }
}

/// Seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Its output's name in the files folder.
pub fn output_name(id: &str) -> String {
    format!("{FOLDER}/{id}.{ENDING}")
}

pub(super) fn record_path(state: &Path, id: &str) -> PathBuf {
    state.join(format!("{id}.json"))
}

pub(super) fn lock_path(state: &Path, id: &str) -> PathBuf {
    state.join(format!("{id}.lock"))
}

pub(super) fn stop_path(state: &Path, id: &str) -> PathBuf {
    state.join(format!("{id}.stop"))
}

/// Record `id` in `state`.
pub fn load(state: &Path, id: &str) -> anyhow::Result<Record> {
    link_machine::machine::check_id(id).map_err(|_| {
        anyhow::anyhow!("{} isn't a background command's id", link_format::clean(id))
    })?;
    let bytes = std::fs::read(record_path(state, id))
        .with_context(|| format!("there's no background command {id}"))?;
    link_files::saved::parse_json(&bytes)
        .with_context(|| format!("reading background command {id}"))
}

/// Writes `record` whole, then moves it in place.
pub fn save(state: &Path, record: &Record) -> anyhow::Result<()> {
    let path = record_path(state, &record.id);
    link_files::write_private(&path, &link_files::saved::print_json(record)?)
        .with_context(|| format!("writing {}", path.display()))
}

/// Every record in `state`, oldest first.
pub fn records(state: &Path) -> Vec<Record> {
    let Ok(entries) = std::fs::read_dir(state) else {
        return Vec::new();
    };
    let mut records: Vec<Record> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let id = name.strip_suffix(".json")?;
            load(state, id).ok()
        })
        .collect();
    records.sort_by(|a, b| (a.started, &a.id).cmp(&(b.started, &b.id)));
    records
}

/// Its lock file, opened.
pub(super) fn open_lock(state: &Path, id: &str) -> anyhow::Result<File> {
    let path = lock_path(state, id);
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))
}

/// Its lock, when no supervisor or sweep holds it.
pub(super) fn take_lock(state: &Path, id: &str) -> Option<File> {
    let lock = open_lock(state, id).ok()?;
    lock.try_lock().ok()?;
    Some(lock)
}

/// The machine's boot, as an id: what a process's group and start time
/// are counted within.
pub fn boot() -> String {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map(|id| id.trim().to_string())
            .unwrap_or_default()
    }
    #[cfg(target_os = "macos")]
    {
        let mut time = libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        let mut size = std::mem::size_of::<libc::timeval>();
        // SAFETY: a sysctl into a value of its size.
        let got = unsafe {
            libc::sysctlbyname(
                c"kern.boottime".as_ptr(),
                (&mut time as *mut libc::timeval).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        match got {
            0 => format!("{}.{}", time.tv_sec, time.tv_usec),
            _ => String::new(),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        String::new()
    }
}

/// When process `pid` started, in its system's own units; none if it's
/// gone.
pub fn started_at(pid: i32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // After the name, which may hold anything, in parentheses: the
        // start time is the 22nd field, the 20th after it.
        let (_, rest) = stat.rsplit_once(')')?;
        rest.split_whitespace().nth(19)?.parse().ok()
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: proc_pidinfo fills a value of the size it's given.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let got = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        (got == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

/// Kills what's left of `record`'s process group, only when it's surely
/// its own: on the same boot, and its leader the one recorded, or gone
/// (a group's id isn't given to another while any of it runs).
pub(super) fn kill_left(record: &Record) {
    let Some(group) = record.group.filter(|g| *g > 1) else {
        return;
    };
    if record.boot.is_empty() || record.boot != boot() {
        return;
    }
    // SAFETY: signal 0 only asks whether the group is there.
    if unsafe { libc::kill(-group, 0) } != 0 {
        return;
    }
    match started_at(group) {
        Some(start) if Some(start) != record.leader_start => {}
        _ => {
            // SAFETY: a signal to a group this command led.
            unsafe { libc::kill(-group, libc::SIGKILL) };
        }
    }
}
