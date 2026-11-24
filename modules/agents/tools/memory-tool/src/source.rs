//! Where a document's text is read from: the index keeps only where each
//! passage is, so its text is read from the agent's file, or, for what
//! memory made itself and no file holds (a document's extracted text, a
//! conversation's copy made from its events), from the index's
//! `derived` table.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

use crate::tree::Files;

/// Document `document`'s bytes, at `path`, no more than `most` of a
/// file: none when its file is gone or larger.
pub(crate) fn whole(
    db: &Connection,
    files: &dyn Files,
    document: i64,
    path: &str,
    most: u64,
) -> Result<Option<Vec<u8>>> {
    let made: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM derived WHERE document = ?1",
            [document],
            |row| row.get(0),
        )
        .optional()?;
    match made {
        Some(body) => Ok(Some(body)),
        None => files.read(path, most),
    }
}

/// Bytes `start` to `end` of document `document`, at `path`: a passage,
/// read without the rest of its file where it's kept on this machine.
pub(crate) fn range(
    db: &Connection,
    files: &dyn Files,
    document: i64,
    path: &str,
    start: u64,
    end: u64,
) -> Result<Option<Vec<u8>>> {
    let made: Option<Vec<u8>> = db
        .query_row(
            "SELECT substr(body, ?2 + 1, ?3 - ?2) FROM derived WHERE document = ?1",
            params![document, start as i64, end as i64],
            |row| row.get(0),
        )
        .optional()?;
    match made {
        Some(bytes) => Ok(Some(bytes)),
        None => files.read_range(path, start, end),
    }
}
