//! SQLite databases a harness or memory keeps: copied whole with SQLite's
//! online backup, never as files beside a live write-ahead log, and
//! their paths rewritten through SQL, never as text.

use link_format::WithinExt;
use std::path::Path;

use anyhow::ensure;
use rusqlite::{Connection, OpenFlags};

/// `live`, an SQLite file another process may be writing, copied to
/// `to` with SQLite's online backup, its write-ahead log in it; tried
/// again for a while when it's busy. Left as it was: one with a log or
/// shared memory beside it is opened read-only, so writes in its log are
/// never folded into it; one without is opened to write, though nothing
/// is written, so SQLite takes away the log and shared memory it makes
/// where it's the last to close, which a read-only one would leave.
#[expect(clippy::disallowed_methods, reason = "backs off before copying again")]
pub fn backup(live: &Path, to: &Path) -> anyhow::Result<()> {
    let beside = |end: &str| {
        let mut name = live.as_os_str().to_owned();
        name.push(end);
        std::path::PathBuf::from(name).exists()
    };
    let read_only = || {
        Connection::open_with_flags(
            live,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
    };
    let source = if beside("-wal") || beside("-shm") {
        read_only()
    } else {
        Connection::open_with_flags(
            live,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .or_else(|_| read_only())
    }?;
    let mut tries = 0;
    loop {
        match source.backup(
            rusqlite::MAIN_DB,
            to,
            None::<fn(rusqlite::backup::Progress)>,
        ) {
            Ok(()) => return Ok(()),
            Err(_) if tries < 50 => {
                tries += 1;
                let _ = std::fs::remove_file(to);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(err) => return Err(err.into()),
        }
    }
}

/// An error unless the database at `path`, a copy, reads whole
/// (`quick_check`). Opened to write, so its log is gone once it's closed.
pub fn check(path: &Path) -> anyhow::Result<()> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    let said: String = db.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    ensure!(said == "ok", "{} isn't whole: {said}", path.display());
    Ok(())
}

/// In the database at `path`, each text value naming a path in a folder
/// of `pairs` names the same path in its second folder instead, whole
/// names only ([`link_settings::folders::replace_paths`]), the first pair
/// first. Every ordinary table's every column; a full-text index follows
/// its table by the table's own triggers, so it and its own tables are
/// left to them. How many values changed.
pub fn rewrite(path: &Path, pairs: &[(String, String)]) -> anyhow::Result<usize> {
    let mut db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .within(format!("opening {}", path.display()))?;
    let tables: Vec<(String, String, String)> = db
        .prepare("SELECT name, type, coalesce(sql, '') FROM sqlite_master WHERE type = 'table'")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let virtuals: Vec<&str> = tables
        .iter()
        .filter(|(_, _, sql)| sql.to_uppercase().starts_with("CREATE VIRTUAL"))
        .map(|(name, ..)| name.as_str())
        .collect();
    let ordinary = |name: &str, sql: &str| {
        !name.starts_with("sqlite_")
            && !sql.to_uppercase().contains("WITHOUT ROWID")
            && !virtuals
                .iter()
                .any(|v| name == *v || name.starts_with(&format!("{v}_")))
    };
    let tx = db.transaction()?;
    let mut changed = 0;
    for (table, _, sql) in &tables {
        if !ordinary(table, sql) {
            continue;
        }
        let quoted = format!("\"{}\"", table.replace('"', "\"\""));
        let columns: Vec<String> = tx
            .prepare(&format!("PRAGMA table_info({quoted})"))?
            .query_map([], |row| row.get(1))?
            .collect::<Result<_, _>>()?;
        for column in columns {
            let col = format!("\"{}\"", column.replace('"', "\"\""));
            for (was, now) in pairs {
                let rows: Vec<(i64, String)> = tx
                    .prepare(&format!(
                        "SELECT rowid, {col} FROM {quoted} \
                         WHERE typeof({col}) = 'text' AND instr({col}, ?1) > 0"
                    ))?
                    .query_map([was], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<_, _>>()?;
                for (rowid, text) in rows {
                    let out = link_settings::folders::replace_paths(&text, was, now);
                    if out != text {
                        tx.execute(
                            &format!("UPDATE {quoted} SET {col} = ?1 WHERE rowid = ?2"),
                            rusqlite::params![out, rowid],
                        )?;
                        changed += 1;
                    }
                }
            }
        }
    }
    tx.commit()?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database's paths are rewritten through SQL, whole names only,
    /// its full-text index following by its triggers; a backup carries
    /// what's only in its write-ahead log yet.
    #[test]
    fn a_databases_paths_are_rewritten_through_sql() {
        let dir = link_test_kit::TempDir::new("databases");
        let live = dir.join("state.db");
        let db = Connection::open(&live).unwrap();
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE messages (id INTEGER PRIMARY KEY, content TEXT, n INTEGER);
             CREATE VIRTUAL TABLE messages_fts USING fts5(content, content=messages, content_rowid=id);
             CREATE TRIGGER m_au AFTER UPDATE ON messages BEGIN
               INSERT INTO messages_fts(messages_fts, rowid, content) VALUES('delete', old.id, old.content);
               INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
             END;
             INSERT INTO messages VALUES (1, 'read /h/.otter/notes.md and /h/.otter-old', 7);
             INSERT INTO messages_fts(rowid, content) SELECT id, content FROM messages;",
        )
        .unwrap();
        // Still open: the row is in the write-ahead log, not the file.
        let copy = dir.join("copy.db");
        backup(&live, &copy).unwrap();
        check(&copy).unwrap();
        let pairs = [(
            "/h/.otter".to_string(),
            "/l/Harnesses/otter/.otter".to_string(),
        )];
        assert_eq!(rewrite(&copy, &pairs).unwrap(), 1);
        let copied = Connection::open(&copy).unwrap();
        let content: String = copied
            .query_row("SELECT content FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            content,
            "read /l/Harnesses/otter/.otter/notes.md and /h/.otter-old"
        );
        let found: i64 = copied
            .query_row(
                "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH '\"Harnesses\"'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(found, 1);
        drop(db);
        assert_eq!(rewrite(&copy, &pairs).unwrap(), 0);
    }
}
