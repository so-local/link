//! `lnk-sqlite-store`: the SQLite store driver. An agent's conversations, their
//! events and their leases, in one file on this machine,
//! `events.sqlite` in the folder its caller names (`LNK_STORE_DIR`, the
//! agent's `chats/`), so it moves with the agent's files. It
//! answers `link_store_contract::store`'s contract: `contract`, `call` with a
//! request on stdin, and `serve` with one a line, as the store bridge
//! keeps it running.
//!
//! It keeps the agent's scheduled jobs too (its contract says `jobs`):
//! each in table `jobs` by its place and ID, and the day's count and the
//! waiting jobs' mark in `job_days`, a row an agent. And its files by
//! path (its contract says `files`): instructions, skills and memory's,
//! each a row of `files` with its version, the next tick of its agent's
//! clock in `file_clocks` at each write, so a file removed and made again
//! never has a version it had before; and the tools each conversation
//! was narrowed to, in `tools`.
//!
//! One writer at a time, of any process: each write is a transaction
//! that takes the file's lock first (`BEGIN IMMEDIATE`), so an event's
//! number is read and the next written while it's held, a lease is read
//! and taken the same way, and a job is found in its place and moved. A
//! reader only reads: it opens the file read-only, and a file that isn't
//! there, or doesn't hold the tables yet, holds nothing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use link_format::{Done, Out, Report};
use link_store_contract::store::{self, Agent, Job, Kept, Lease, Place, Reply, Request, Took};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

/// The file, in the folder its caller names.
const FILE: &str = "events.sqlite";

/// How long a call waits for another's write to end.
const BUSY: Duration = Duration::from_secs(10);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS events (
    owner TEXT NOT NULL,
    agent TEXT NOT NULL,
    thread TEXT NOT NULL,
    seq INTEGER NOT NULL,
    event TEXT NOT NULL,
    PRIMARY KEY (owner, agent, thread, seq)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS leases (
    owner TEXT NOT NULL,
    agent TEXT NOT NULL,
    thread TEXT NOT NULL,
    holder TEXT NOT NULL,
    started INTEGER NOT NULL,
    renewed_ms INTEGER NOT NULL,
    PRIMARY KEY (owner, agent, thread)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS jobs (
    owner TEXT NOT NULL,
    agent TEXT NOT NULL,
    place TEXT NOT NULL,
    id TEXT NOT NULL,
    spec TEXT NOT NULL,
    due INTEGER NOT NULL,
    PRIMARY KEY (owner, agent, place, id)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS files (
    owner TEXT NOT NULL,
    agent TEXT NOT NULL,
    path TEXT NOT NULL,
    body BLOB NOT NULL,
    version INTEGER NOT NULL,
    PRIMARY KEY (owner, agent, path)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS file_clocks (
    owner TEXT NOT NULL,
    agent TEXT NOT NULL,
    version INTEGER NOT NULL,
    PRIMARY KEY (owner, agent)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS tools (
    owner TEXT NOT NULL,
    agent TEXT NOT NULL,
    thread TEXT NOT NULL,
    tools TEXT NOT NULL,
    PRIMARY KEY (owner, agent, thread)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS job_days (
    owner TEXT NOT NULL,
    agent TEXT NOT NULL,
    day INTEGER NOT NULL DEFAULT 0,
    sent INTEGER NOT NULL DEFAULT 0,
    mark INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (owner, agent)
) WITHOUT ROWID;
";

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(try_main)
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    let Some(operation) = link_sqlite_store::cli::operation().map_err(|err| {
        Report::error(link_format::sentence!("Couldn't uninstall it.")).cause(err)
    })?
    else {
        return Ok(Done::shown());
    };
    store::run(
        out,
        operation,
        store::Contract {
            jobs: true,
            files: true,
            ..store::Contract::serving()
        },
        answer,
    )
}

/// The folder it keeps its file in: absolute, and not a link.
fn dir() -> anyhow::Result<PathBuf> {
    let dir = std::env::var_os(store::DIR_ENV)
        .map(PathBuf::from)
        .with_context(|| format!("{} isn't set: a harness runs it", store::DIR_ENV))?;
    anyhow::ensure!(dir.is_absolute(), "{} isn't absolute", store::DIR_ENV);
    Ok(dir)
}

/// The file in `dir`, to write: made only the user's when it's new.
fn writing(dir: &Path) -> anyhow::Result<Connection> {
    link_files::create_private_dir(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(FILE);
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Made, never followed through a link, before SQLite opens it.
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
    }
    let db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", path.display()))?;
    harden(&db)?;
    db.busy_timeout(BUSY)?;
    db.execute_batch(SCHEMA)?;
    Ok(db)
}

/// Trusts nothing of the file's own: it's in a folder a harness's sandbox
/// writes, and the driver may run outside it, behind the store bridge. No
/// function or view of its schema runs, and SQL can't corrupt it on
/// purpose (SQLite's defensive mode).
fn harden(db: &Connection) -> anyhow::Result<()> {
    use rusqlite::config::DbConfig;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    Ok(())
}

/// The file in `dir`, to read: none when it isn't there, or doesn't
/// hold the conversations' tables yet.
fn reading(dir: &Path) -> anyhow::Result<Option<Connection>> {
    reading_tables(dir, &["events", "leases"])
}

/// The file in `dir`, to read jobs: none when it isn't there, or doesn't
/// hold the jobs' tables yet.
fn reading_jobs(dir: &Path) -> anyhow::Result<Option<Connection>> {
    reading_tables(dir, &["jobs", "job_days"])
}

/// The file in `dir`, to read: none when it isn't there, or doesn't hold
/// every one of `tables`.
fn reading_tables(dir: &Path, tables: &[&str]) -> anyhow::Result<Option<Connection>> {
    let path = dir.join(FILE);
    match std::fs::symlink_metadata(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        Ok(meta) if !meta.is_file() => bail!("{} isn't a file", path.display()),
        Ok(_) => {}
    }
    let db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", path.display()))?;
    harden(&db)?;
    db.busy_timeout(BUSY)?;
    // A file made but never written yet holds no tables.
    let mut made = true;
    for table in tables {
        made &= db.query_row(
            "SELECT count(*) = 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |row| row.get::<_, bool>(0),
        )?;
    }
    Ok(made.then_some(db))
}

/// Now, in milliseconds since 1970.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn last(db: &Connection, agent: &Agent, thread: &str) -> rusqlite::Result<u64> {
    db.query_row(
        "SELECT coalesce(max(seq), 0) FROM events WHERE owner = ?1 AND agent = ?2 AND thread = ?3",
        params![agent.owner, agent.id, thread],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n as u64)
}

/// A place as the `jobs` table keeps it.
fn place(place: Place) -> &'static str {
    match place {
        Place::Waiting => "waiting",
        Place::Firing => "firing",
        Place::Ending => "ending",
        Place::Ended => "ended",
    }
}

/// A job from its row: `id`, `spec`, `due`.
fn job_of(row: &rusqlite::Row) -> rusqlite::Result<(String, String, i64)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}

fn job((id, spec, due): (String, String, i64)) -> anyhow::Result<Job> {
    Ok(Job {
        id,
        spec: serde_json::from_str(&spec).context("reading a job")?,
        due: due as u64,
    })
}

/// Moves `agent`'s waiting jobs' mark on, in `tx`.
fn marked(tx: &Connection, agent: &Agent) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO job_days (owner, agent, mark) VALUES (?1, ?2, 1) \
         ON CONFLICT (owner, agent) DO UPDATE SET mark = mark + 1",
        params![agent.owner, agent.id],
    )
    .map(drop)
}

/// Answers `request`.
fn answer(request: Request) -> anyhow::Result<Reply> {
    let dir = dir()?;
    if request.of_jobs() {
        return jobs(&dir, request);
    }
    if request.of_files() {
        return files(&dir, request);
    }
    Ok(match request {
        Request::Append {
            agent,
            thread,
            event,
        } => {
            let seq = store::seq_of(&event).context("an event without its number")?;
            let text = serde_json::to_string(&event)?;
            anyhow::ensure!(
                text.len() as u64 <= store::EVENT_MAX,
                "the event is larger than {} bytes",
                store::EVENT_MAX
            );
            let mut db = writing(&dir)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let last = last(&tx, &agent, &thread)?;
            if last.checked_add(1) != Some(seq) {
                return Ok(Reply::Taken { last });
            }
            tx.execute(
                "INSERT INTO events (owner, agent, thread, seq, event) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![agent.owner, agent.id, thread, seq as i64, text],
            )?;
            tx.commit()?;
            Reply::Done
        }
        Request::Read {
            agent,
            thread,
            first,
        } => {
            let Some(db) = reading(&dir)? else {
                return Ok(Reply::Events { events: Vec::new() });
            };
            let mut statement = db.prepare(
                "SELECT event FROM events WHERE owner = ?1 AND agent = ?2 AND thread = ?3 \
                 AND seq >= ?4 ORDER BY seq",
            )?;
            let first = i64::try_from(first).unwrap_or(i64::MAX);
            let rows = statement
                .query_map(params![agent.owner, agent.id, thread, first], |row| {
                    row.get::<_, String>(0)
                })?;
            let mut events = Vec::new();
            for row in rows {
                events.push(serde_json::from_str(&row?).context("reading an event")?);
            }
            Reply::Events { events }
        }
        Request::Last { agent, thread } => Reply::Last {
            last: match reading(&dir)? {
                Some(db) => last(&db, &agent, &thread)?,
                None => 0,
            },
        },
        Request::Threads { agent } => {
            let Some(db) = reading(&dir)? else {
                return Ok(Reply::Threads {
                    threads: Vec::new(),
                });
            };
            let mut statement = db.prepare(
                "SELECT thread, max(seq) FROM events WHERE owner = ?1 AND agent = ?2 \
                 GROUP BY thread ORDER BY thread",
            )?;
            let threads = statement
                .query_map(params![agent.owner, agent.id], |row| {
                    Ok(Kept {
                        thread: row.get(0)?,
                        last: row.get::<_, i64>(1)? as u64,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
            Reply::Threads { threads }
        }
        Request::Remove { agent, thread } => {
            if reading(&dir)?.is_some() {
                let mut db = writing(&dir)?;
                let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                for table in ["events", "leases"] {
                    tx.execute(
                        &format!(
                            "DELETE FROM {table} WHERE owner = ?1 AND agent = ?2 AND thread = ?3"
                        ),
                        params![agent.owner, agent.id, thread],
                    )?;
                }
                tx.commit()?;
            }
            Reply::Done
        }
        Request::Take {
            agent,
            thread,
            holder,
            lapse_ms,
        } => {
            let mut db = writing(&dir)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let renewed: Option<i64> = tx
                .query_row(
                    "SELECT renewed_ms FROM leases WHERE owner = ?1 AND agent = ?2 AND thread = ?3",
                    params![agent.owner, agent.id, thread],
                    |row| row.get(0),
                )
                .optional()?;
            let now = now_ms();
            let took = match renewed {
                None => Took::Took,
                Some(renewed) if now.saturating_sub(renewed as u64) < lapse_ms => {
                    return Ok(Reply::Took { took: Took::Held });
                }
                Some(_) => Took::Lapsed,
            };
            tx.execute(
                "INSERT OR REPLACE INTO leases (owner, agent, thread, holder, started, renewed_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    agent.owner,
                    agent.id,
                    thread,
                    holder,
                    (now / 1000) as i64,
                    now as i64
                ],
            )?;
            tx.commit()?;
            Reply::Took { took }
        }
        Request::Renew {
            agent,
            thread,
            holder,
        } => {
            let db = writing(&dir)?;
            let renewed = db.execute(
                "UPDATE leases SET renewed_ms = ?5 \
                 WHERE owner = ?1 AND agent = ?2 AND thread = ?3 AND holder = ?4",
                params![agent.owner, agent.id, thread, holder, now_ms() as i64],
            )?;
            match renewed {
                0 => Reply::Lost,
                _ => Reply::Done,
            }
        }
        Request::End {
            agent,
            thread,
            holder,
        } => {
            if reading(&dir)?.is_some() {
                writing(&dir)?.execute(
                    "DELETE FROM leases \
                     WHERE owner = ?1 AND agent = ?2 AND thread = ?3 AND holder = ?4",
                    params![agent.owner, agent.id, thread, holder],
                )?;
            }
            Reply::Done
        }
        Request::Leases { agent, lapse_ms } => {
            let Some(db) = reading(&dir)? else {
                return Ok(Reply::Leases { leases: Vec::new() });
            };
            let now = now_ms();
            let mut statement = db.prepare(
                "SELECT thread, holder, started, renewed_ms FROM leases \
                 WHERE owner = ?1 AND agent = ?2 ORDER BY thread",
            )?;
            let leases = statement
                .query_map(params![agent.owner, agent.id], |row| {
                    let renewed = row.get::<_, i64>(3)? as u64;
                    Ok(Lease {
                        thread: row.get(0)?,
                        holder: row.get(1)?,
                        started: row.get::<_, i64>(2)? as u64,
                        renewed: renewed / 1000,
                        lapsed: now.saturating_sub(renewed) >= lapse_ms,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
            Reply::Leases { leases }
        }
        Request::ToolsRead { agent, thread } => Reply::Tools {
            tools: match reading_tables(&dir, &["tools"])? {
                Some(db) => read_tools(&db, &agent, &thread)?,
                None => None,
            },
        },
        Request::ToolsList { agent } => Reply::Narrowed {
            narrowed: match reading_tables(&dir, &["tools"])? {
                Some(db) => {
                    let mut query = db.prepare(
                        "SELECT thread, tools FROM tools WHERE owner = ?1 AND agent = ?2",
                    )?;
                    let rows = query
                        .query_map(params![agent.owner, agent.id], |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    rows.into_iter()
                        .map(|(thread, tools)| {
                            Ok((thread, store::folder::read_tools(tools.as_bytes())?))
                        })
                        .collect::<anyhow::Result<_>>()?
                }
                None => Default::default(),
            },
        },
        Request::ToolsNarrow {
            agent,
            thread,
            tools,
        } => {
            store::check_tools(&tools)?;
            let mut db = writing(&dir)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let narrowed: Vec<String> = match read_tools(&tx, &agent, &thread)? {
                Some(had) => tools.into_iter().filter(|t| had.contains(t)).collect(),
                None => tools,
            };
            tx.execute(
                "INSERT OR REPLACE INTO tools (owner, agent, thread, tools) VALUES (?1, ?2, ?3, ?4)",
                params![
                    agent.owner,
                    agent.id,
                    thread,
                    serde_json::to_string(&narrowed)?
                ],
            )?;
            tx.commit()?;
            Reply::Tools {
                tools: Some(narrowed),
            }
        }
        _ => bail!("not a conversation's request"),
    })
}

/// The tools `agent`'s conversation `thread` was narrowed to: none for
/// every tool. A list that isn't one is an error, never every tool.
fn read_tools(db: &Connection, agent: &Agent, thread: &str) -> anyhow::Result<Option<Vec<String>>> {
    let text: Option<String> = db
        .query_row(
            "SELECT tools FROM tools WHERE owner = ?1 AND agent = ?2 AND thread = ?3",
            params![agent.owner, agent.id, thread],
            |row| row.get(0),
        )
        .optional()?;
    text.map(|t| store::folder::read_tools(t.as_bytes()))
        .transpose()
}

/// `agent`'s file at `path`: its bytes and version.
fn file(db: &Connection, agent: &Agent, path: &str) -> rusqlite::Result<Option<(Vec<u8>, u64)>> {
    db.query_row(
        "SELECT body, version FROM files WHERE owner = ?1 AND agent = ?2 AND path = ?3",
        params![agent.owner, agent.id, path],
        |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u64)),
    )
    .optional()
}

/// The next tick of `agent`'s file clock, a file's version at its next
/// write: past every version its files have had, a removed one's too.
/// Its first tick is past every version its files have now.
fn tick(tx: &rusqlite::Transaction, agent: &Agent) -> anyhow::Result<u64> {
    let had: Option<i64> = tx
        .query_row(
            "SELECT version FROM file_clocks WHERE owner = ?1 AND agent = ?2",
            params![agent.owner, agent.id],
            |row| row.get(0),
        )
        .optional()?;
    let next = match had {
        Some(version) => version + 1,
        None => tx.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM files WHERE owner = ?1 AND agent = ?2",
            params![agent.owner, agent.id],
            |row| row.get(0),
        )?,
    };
    tx.execute(
        "INSERT OR REPLACE INTO file_clocks (owner, agent, version) VALUES (?1, ?2, ?3)",
        params![agent.owner, agent.id, next],
    )?;
    Ok(next as u64)
}

/// Answers `request`, of the agent's files by path.
fn files(dir: &Path, request: Request) -> anyhow::Result<Reply> {
    use base64::Engine as _;
    Ok(match request {
        Request::FileRead { agent, path } => {
            store::check_file_path(&path, false)?;
            let Some(db) = reading_tables(dir, &["files"])? else {
                return Ok(Reply::File { file: None });
            };
            Reply::File {
                file: file(&db, &agent, &path)?.map(|(body, version)| store::Stored {
                    data: base64::engine::general_purpose::STANDARD.encode(body),
                    version,
                }),
            }
        }
        Request::FileList { agent, folder } => {
            store::check_file_path(&folder, true)?;
            let prefix = match folder.ends_with('/') {
                true => folder,
                false => format!("{folder}/"),
            };
            let Some(db) = reading_tables(dir, &["files"])? else {
                return Ok(Reply::Files { files: Vec::new() });
            };
            let mut statement = db.prepare(
                "SELECT path FROM files WHERE owner = ?1 AND agent = ?2 \
                 AND substr(path, 1, length(?3)) = ?3 ORDER BY path LIMIT ?4",
            )?;
            let files: Vec<String> = statement
                .query_map(
                    params![agent.owner, agent.id, prefix, store::FILES_MAX as i64 + 1],
                    |row| row.get(0),
                )?
                .collect::<rusqlite::Result<_>>()?;
            anyhow::ensure!(
                files.len() <= store::FILES_MAX,
                "{prefix} holds more than {} files",
                store::FILES_MAX
            );
            Reply::Files { files }
        }
        Request::FileWrite {
            agent,
            path,
            data,
            version,
        } => {
            store::check_file_path(&path, false)?;
            let body = store::file_data(&path, &data)?;
            let mut db = writing(dir)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let now = file(&tx, &agent, &path)?.map_or(0, |(_, v)| v);
            if now != version {
                return Ok(Reply::Stale { version: now });
            }
            let next = tick(&tx, &agent)?;
            tx.execute(
                "INSERT OR REPLACE INTO files (owner, agent, path, body, version) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![agent.owner, agent.id, path, body, next as i64],
            )?;
            tx.commit()?;
            Reply::Wrote { version: next }
        }
        Request::FileRemove {
            agent,
            path,
            version,
        } => {
            store::check_file_path(&path, false)?;
            if reading_tables(dir, &["files"])?.is_none() {
                return Ok(Reply::Found { found: false });
            }
            let removed = writing(dir)?.execute(
                "DELETE FROM files WHERE owner = ?1 AND agent = ?2 AND path = ?3 AND version = ?4",
                params![agent.owner, agent.id, path, version as i64],
            )?;
            Reply::Found { found: removed > 0 }
        }
        Request::FileAppend { agent, path, data } => {
            store::check_file_path(&path, false)?;
            let more = store::file_data(&path, &data)?;
            let mut db = writing(dir)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let (mut body, _) = file(&tx, &agent, &path)?.unwrap_or_default();
            anyhow::ensure!(
                (body.len() + more.len()) as u64 <= store::file_max(&path),
                "{path} would be larger than {} bytes",
                store::file_max(&path)
            );
            body.extend_from_slice(&more);
            let next = tick(&tx, &agent)?;
            tx.execute(
                "INSERT OR REPLACE INTO files (owner, agent, path, body, version) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![agent.owner, agent.id, path, body, next as i64],
            )?;
            tx.commit()?;
            Reply::Wrote { version: next }
        }
        _ => bail!("not a file's request"),
    })
}

/// Answers `request`, of the agent's jobs.
fn jobs(dir: &Path, request: Request) -> anyhow::Result<Reply> {
    Ok(match request {
        Request::JobPut {
            agent,
            place: at,
            job,
        } => {
            store::check_job(&job)?;
            let mut db = writing(dir)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute(
                "INSERT OR REPLACE INTO jobs (owner, agent, place, id, spec, due) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    agent.owner,
                    agent.id,
                    place(at),
                    job.id,
                    serde_json::to_string(&job.spec)?,
                    i64::try_from(job.due).unwrap_or(i64::MAX)
                ],
            )?;
            if at == Place::Waiting {
                marked(&tx, &agent)?;
            }
            tx.commit()?;
            Reply::Done
        }
        Request::JobGet {
            agent,
            place: at,
            id,
        } => {
            let Some(db) = reading_jobs(dir)? else {
                return Ok(Reply::Job { job: None });
            };
            let row = db
                .query_row(
                    "SELECT id, spec, due FROM jobs \
                     WHERE owner = ?1 AND agent = ?2 AND place = ?3 AND id = ?4",
                    params![agent.owner, agent.id, place(at), id],
                    job_of,
                )
                .optional()?;
            Reply::Job {
                job: row.map(job).transpose()?,
            }
        }
        Request::JobList { agent, place: at } => {
            let Some(db) = reading_jobs(dir)? else {
                return Ok(Reply::Jobs { jobs: Vec::new() });
            };
            let mut statement = db.prepare(
                "SELECT id, spec, due FROM jobs WHERE owner = ?1 AND agent = ?2 AND place = ?3",
            )?;
            let rows = statement.query_map(params![agent.owner, agent.id, place(at)], job_of)?;
            let mut jobs = Vec::new();
            for row in rows {
                jobs.push(job(row?)?);
            }
            Reply::Jobs { jobs }
        }
        Request::JobCount { agent, place: at } => Reply::Count {
            count: match reading_jobs(dir)? {
                Some(db) => db.query_row(
                    "SELECT count(*) FROM jobs WHERE owner = ?1 AND agent = ?2 AND place = ?3",
                    params![agent.owner, agent.id, place(at)],
                    |row| row.get::<_, i64>(0),
                )? as u64,
                None => 0,
            },
        },
        Request::JobShift {
            agent,
            id,
            from,
            to,
        } => {
            if reading_jobs(dir)?.is_none() {
                return Ok(Reply::Found { found: false });
            }
            let mut db = writing(dir)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let there: bool = tx.query_row(
                "SELECT count(*) = 1 FROM jobs \
                 WHERE owner = ?1 AND agent = ?2 AND place = ?3 AND id = ?4",
                params![agent.owner, agent.id, place(from), id],
                |row| row.get(0),
            )?;
            if !there {
                return Ok(Reply::Found { found: false });
            }
            if from != to {
                tx.execute(
                    "DELETE FROM jobs WHERE owner = ?1 AND agent = ?2 AND place = ?3 AND id = ?4",
                    params![agent.owner, agent.id, place(to), id],
                )?;
                tx.execute(
                    "UPDATE jobs SET place = ?4 \
                     WHERE owner = ?1 AND agent = ?2 AND place = ?3 AND id = ?5",
                    params![agent.owner, agent.id, place(from), place(to), id],
                )?;
                if from == Place::Waiting || to == Place::Waiting {
                    marked(&tx, &agent)?;
                }
            }
            tx.commit()?;
            Reply::Found { found: true }
        }
        Request::JobRemove {
            agent,
            place: at,
            id,
        } => {
            if reading_jobs(dir)?.is_none() {
                return Ok(Reply::Found { found: false });
            }
            let mut db = writing(dir)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let removed = tx.execute(
                "DELETE FROM jobs WHERE owner = ?1 AND agent = ?2 AND place = ?3 AND id = ?4",
                params![agent.owner, agent.id, place(at), id],
            )?;
            if removed > 0 && at == Place::Waiting {
                marked(&tx, &agent)?;
            }
            tx.commit()?;
            Reply::Found { found: removed > 0 }
        }
        Request::JobNext { agent } => Reply::Next {
            next: match reading_jobs(dir)? {
                Some(db) => db
                    .query_row(
                        "SELECT min(due) FROM jobs \
                         WHERE owner = ?1 AND agent = ?2 AND place = 'waiting'",
                        params![agent.owner, agent.id],
                        |row| row.get::<_, Option<i64>>(0),
                    )?
                    .map(|due| due as u64),
                None => None,
            },
        },
        Request::JobDay { agent } => {
            let day = match reading_jobs(dir)? {
                Some(db) => db
                    .query_row(
                        "SELECT day, sent FROM job_days WHERE owner = ?1 AND agent = ?2",
                        params![agent.owner, agent.id],
                        |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u32)),
                    )
                    .optional()?,
                None => None,
            };
            let (day, sent) = day.unwrap_or_default();
            Reply::Day { day, sent }
        }
        Request::JobSetDay { agent, day, sent } => {
            writing(dir)?.execute(
                "INSERT INTO job_days (owner, agent, day, sent) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT (owner, agent) DO UPDATE SET day = ?3, sent = ?4",
                params![
                    agent.owner,
                    agent.id,
                    i64::try_from(day).unwrap_or(i64::MAX),
                    sent
                ],
            )?;
            Reply::Done
        }
        Request::JobMark { agent } => Reply::Mark {
            mark: match reading_jobs(dir)? {
                Some(db) => db
                    .query_row(
                        "SELECT mark FROM job_days WHERE owner = ?1 AND agent = ?2",
                        params![agent.owner, agent.id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?
                    .map_or(0, |mark| mark as u64),
                None => 0,
            },
        },
        _ => bail!("not a job's request"),
    })
}

#[cfg(test)]
link_test_kit::program!(unit);
