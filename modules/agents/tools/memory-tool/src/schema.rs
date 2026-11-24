//! The index's file: opened defensively, its tables made, and a copy of
//! it checked before it's used.

use super::*;

/// Opens the index `file`, trusting nothing of its own ([`harden`]).
pub fn open_file(file: &Path) -> Result<Connection> {
    let db =
        Connection::open(file).with_context(|| format!("opening the index {}", file.display()))?;
    harden(&db)?;
    Ok(db)
}

/// Trusts nothing of the index's own: it may be a copy fetched from a
/// bucket its agent writes, opened in a process serving every agent. No
/// function or view of its schema runs, and SQL can't corrupt it on
/// purpose (SQLite's defensive mode).
pub(super) fn harden(db: &Connection) -> Result<()> {
    use rusqlite::config::DbConfig;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    Ok(())
}

/// Makes what of the index's schema `db` lacks. A document or a passage
/// never takes an ID one had before (`AUTOINCREMENT`): its words, in
/// Tantivy, are named by it.
pub(super) fn create(db: &Connection) -> Result<()> {
    let queued = QUEUED.replace("{QUEUE_LATER}", &QUEUE_LATER.to_string());
    db.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS documents (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 path TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL,
                 title TEXT NOT NULL,
                 size INTEGER NOT NULL,
                 modified INTEGER NOT NULL,
                 added INTEGER NOT NULL DEFAULT 0,
                 hash INTEGER NOT NULL DEFAULT 0,
                 since_kind TEXT,
                 since INTEGER,
                 changed INTEGER GENERATED ALWAYS AS (max(modified, added)) VIRTUAL
             );
             CREATE INDEX IF NOT EXISTS documents_changed ON documents(changed DESC, path);
             CREATE TABLE IF NOT EXISTS removed (
                 path TEXT PRIMARY KEY,
                 kind TEXT NOT NULL,
                 title TEXT NOT NULL,
                 at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS passages (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 document INTEGER NOT NULL,
                 start INTEGER NOT NULL,
                 end INTEGER NOT NULL,
                 line INTEGER NOT NULL,
                 hash INTEGER NOT NULL,
                 slot INTEGER
             );
             CREATE UNIQUE INDEX IF NOT EXISTS passages_slot ON passages(slot)
                 WHERE slot IS NOT NULL;
             CREATE TABLE IF NOT EXISTS free_slots (slot INTEGER PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS vectors (
                 model TEXT NOT NULL,
                 dimensions INTEGER NOT NULL,
                 cut TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS passages_document ON passages(document, start);
             CREATE TABLE IF NOT EXISTS queue (
                 passage INTEGER PRIMARY KEY,
                 priority INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS queue_order ON queue(priority, passage);
             CREATE TABLE IF NOT EXISTS derived (
                 document INTEGER PRIMARY KEY,
                 body BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS links (
                 document INTEGER NOT NULL,
                 target TEXT NOT NULL,
                 wiki INTEGER NOT NULL,
                 line INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS links_document ON links(document);
             CREATE INDEX IF NOT EXISTS links_target ON links(target);
             CREATE TABLE IF NOT EXISTS unread (
                 path TEXT PRIMARY KEY,
                 why TEXT NOT NULL,
                 attempts INTEGER NOT NULL DEFAULT 0
             );
             CREATE TRIGGER IF NOT EXISTS passages_added AFTER INSERT ON passages
                 BEGIN {COUNT_ADDED} {queued} END;
             CREATE TRIGGER IF NOT EXISTS passages_removed AFTER DELETE ON passages
                 BEGIN {COUNT_REMOVED} {UNQUEUED} {FREED_GONE} END;
             CREATE TRIGGER IF NOT EXISTS passages_changed
                 AFTER UPDATE OF document, slot ON passages
                 BEGIN {COUNT_EMBEDDED} {queued} {FREED} END;
             PRAGMA user_version = {SCHEMA};"
    ))?;
    Ok(())
}

/// An error unless the index `file` (a copy fetched, not yet opened) is
/// whole and holds only the schema [`Index::open_tree`] makes: no view,
/// trigger, table or index of its own, and none made otherwise.
pub fn check_copy(file: &Path) -> Result<()> {
    let db = open_file(file)?;
    let checked: String = db.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if checked != "ok" {
        bail!("the index isn't whole: {}", link_format::clean(&checked));
    }
    let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version != SCHEMA {
        bail!("the index is of schema {version}, not {SCHEMA}");
    }
    let made = Connection::open_in_memory()?;
    harden(&made)?;
    create(&made)?;
    let schema = |db: &Connection| -> Result<HashSet<Vec<Option<String>>>> {
        let mut rows = db.prepare("SELECT type, name, tbl_name, sql FROM sqlite_master")?;
        let rows = rows
            .query_map([], |row| {
                (0..4).map(|i| row.get(i)).collect::<rusqlite::Result<_>>()
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    };
    let expected = schema(&made)?;
    if let Some(odd) = schema(&db)?.into_iter().find(|row| !expected.contains(row)) {
        bail!(
            "the index holds a schema of its own: {}",
            link_format::clean(&format!("{odd:?}"))
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The index's connection trusts nothing of its file: its schema
    /// can't be written as a table, and none of its own functions run.
    #[test]
    fn the_index_is_opened_defensively() {
        let dir = link_test_kit::TempDir::new("index-defensive");
        let index = Index::open(&dir, Some(&dir.join("index.sqlite"))).unwrap();
        let written = index.db.execute_batch(
            "PRAGMA writable_schema = ON;
             UPDATE sqlite_master SET sql = 'CREATE TABLE meta (x)' WHERE name = 'meta';",
        );
        assert!(written.is_err(), "the schema was written");
        let trusted: i64 = index
            .db
            .query_row("PRAGMA trusted_schema", [], |row| row.get(0))
            .unwrap();
        assert_eq!(trusted, 0);
    }
}
