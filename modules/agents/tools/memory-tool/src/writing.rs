//! A document's passages written to the index ([`write_body`]), and
//! what a conversation grew by ([`grow`]): its passages compared by hash,
//! so only those new or changed are written.

use super::*;
use crate::index::words::{Change, Changes};

/// What one batch writes through: its transaction, and the changes to
/// the words it makes, committed with it
/// ([`crate::index::words::Words::commit`]).
pub(crate) struct Batch<'t> {
    pub(crate) db: &'t Connection,
    pub(crate) words: &'t mut Changes,
}
use crate::refreshing::{Titled, title};

/// Document `id`'s text, `body` (at `path`, of `kind`), written: its
/// title, its passages and its links, their words noted in `batch`, and
/// the text itself only where memory made it (`derived`), no file holding
/// it. A passage
/// that didn't change, known by its hash, stays as it is, its words and
/// its vector with it, moved to where it is now: an edit to one line of
/// a long file rewrites only the passage it's in. Each passage was
/// embedded with the title in front, so only while it's the same.
pub(crate) fn write_body(
    batch: &mut Batch<'_>,
    id: i64,
    path: &str,
    kind: Kind,
    body: &[u8],
    derived: bool,
    titled: Titled,
) -> Result<()> {
    let db = batch.db;
    let text = String::from_utf8_lossy(body);
    let title = title(path, kind, &text, titled);
    let before: String =
        db.query_row("SELECT title FROM documents WHERE id = ?1", [id], |row| {
            row.get(0)
        })?;
    let retitled = before != title;
    match derived {
        true => db.execute(
            "INSERT OR REPLACE INTO derived (document, body) VALUES (?1, ?2)",
            params![id, body],
        )?,
        false => db.execute("DELETE FROM derived WHERE document = ?1", [id])?,
    };
    db.execute(
        "UPDATE documents SET title = ?2, hash = ?3 WHERE id = ?1",
        params![id, title, semantic::hash(body)],
    )?;
    // Its title's words, found apart from its passages'.
    if retitled || !titled_before(db, id)? {
        batch.words.push(Change::Title {
            document: id,
            title: title.clone(),
        });
    }
    // What it held, by hash, the first first.
    let mut kept: HashMap<i64, Vec<(i64, usize, usize, usize)>> = HashMap::new();
    {
        let mut statement = db.prepare(
            "SELECT id, start, end, line, hash FROM passages WHERE document = ?1 ORDER BY start",
        )?;
        let rows = statement.query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)? as usize,
                row.get::<_, i64>(3)? as usize,
                row.get::<_, i64>(4)?,
            ))
        })?;
        for row in rows {
            let (passage, start, end, line, hash) = row?;
            kept.entry(hash)
                .or_default()
                .push((passage, start, end, line));
        }
    }
    if retitled {
        db.execute("UPDATE passages SET slot = NULL WHERE document = ?1", [id])?;
    }
    let mut spans = semantic::passages(body);
    // One with no text is found by its title.
    if spans.is_empty() {
        spans.push(semantic::Passage {
            start: 0,
            end: 0,
            line: 1,
        });
    }
    let words_of = |span: &semantic::Passage| String::from_utf8_lossy(&body[span.start..span.end]);
    for span in &spans {
        let hash = semantic::hash(&body[span.start..span.end]);
        let was = kept
            .get_mut(&hash)
            .and_then(|same| (!same.is_empty()).then(|| same.remove(0)));
        match was {
            Some((passage, start, end, line)) => {
                if (start, end, line) != (span.start, span.end, span.line) {
                    db.execute(
                        "UPDATE passages SET start = ?2, end = ?3, line = ?4 WHERE id = ?1",
                        params![
                            passage,
                            span.start as i64,
                            span.end as i64,
                            span.line as i64
                        ],
                    )?;
                }
            }
            None => {
                db.execute(
                    "INSERT INTO passages (document, start, end, line, hash)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        id,
                        span.start as i64,
                        span.end as i64,
                        span.line as i64,
                        hash
                    ],
                )?;
                batch.words.push(Change::Passage {
                    passage: db.last_insert_rowid(),
                    document: id,
                    text: words_of(span).into_owned(),
                });
            }
        }
    }
    for (passage, ..) in kept.into_values().flatten() {
        batch.words.push(Change::PassageGone(passage));
        db.execute("DELETE FROM passages WHERE id = ?1", [passage])?;
    }
    db.execute("DELETE FROM links WHERE document = ?1", [id])?;
    insert_links(db, id, path, &text)?;
    Ok(())
}

/// Whether document `id`'s title is in the words yet: written with its
/// first text.
fn titled_before(db: &Connection, id: i64) -> Result<bool> {
    Ok(db
        .query_row(
            "SELECT 1 FROM passages WHERE document = ?1 LIMIT 1",
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Writes what conversation `id` (at `path`, as `file` found it) grew
/// by since it was indexed, read from where its last read ended
/// ([`tree::Tree::read_since`]): its last passage written again with what
/// follows it, and the passages and links after, the rest as it was.
/// Whether it could: one indexed whole never, or written again before
/// its last passage (that passage's hash differs), is read whole.
pub(crate) fn grow(
    batch: &mut Batch<'_>,
    tree: &mut dyn tree::Tree,
    id: i64,
    path: &str,
    file: &tree::Found,
) -> Result<bool> {
    let db = batch.db;
    let kept: Option<(Option<String>, Option<i64>, i64)> = db
        .query_row(
            "SELECT since_kind, since, hash FROM documents WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((Some(kind), Some(number), hash_before)) = kept else {
        return Ok(false);
    };
    let Some(since) = tree::Since::of(&kind, number) else {
        return Ok(false);
    };
    let last: Option<(i64, i64, i64, i64, i64)> = db
        .query_row(
            "SELECT id, start, end, line, hash FROM passages WHERE document = ?1
             ORDER BY start DESC LIMIT 1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let Some((last, start, end, line, last_hash)) = last else {
        return Ok(false);
    };
    let (start, end) = (start as usize, end as usize);
    let derived = db
        .query_row(
            "SELECT 1 FROM derived WHERE document = ?1",
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    // From its last passage's start: a file's own bytes from there, or the
    // copy kept from there and the sections its new events make.
    let (asked, grew_at, tail) = match (since, derived) {
        (tree::Since::Byte(read), false) => (
            tree::Since::Byte(start as u64),
            (read as usize).saturating_sub(start),
            Vec::new(),
        ),
        (tree::Since::Event(_), true) => {
            let tail: Vec<u8> = db.query_row(
                "SELECT substr(body, ?2 + 1) FROM derived WHERE document = ?1",
                params![id, start as i64],
                |row| row.get(0),
            )?;
            (since, tail.len(), tail)
        }
        _ => return Ok(false),
    };
    let Some(read) = tree.read_since(path, file, asked)? else {
        return Ok(false);
    };
    let (Some(now), None) = (read.since, &read.unreadable) else {
        return Ok(false);
    };
    // What it grew by starts where what was indexed of it ended.
    let bytes = match derived {
        false => read.bytes,
        true => [tail, read.bytes].concat(),
    };
    if bytes.len() < grew_at
        || bytes.len() < end - start
        || semantic::hash(&bytes[..end - start]) != last_hash
    {
        return Ok(false);
    }
    let spans = semantic::passages(&bytes);
    if spans.is_empty() {
        return Ok(false);
    }
    if derived {
        db.execute(
            "UPDATE derived SET body = CAST(body || ?2 AS BLOB) WHERE document = ?1",
            params![id, &bytes[grew_at..]],
        )?;
    }
    // Its last passage, as it was or written again with what follows.
    let same = spans[0].start == 0 && spans[0].end == end - start;
    if !same {
        batch.words.push(Change::PassageGone(last));
        db.execute("DELETE FROM passages WHERE id = ?1", [last])?;
    }
    for span in spans.iter().skip(usize::from(same)) {
        let piece = &bytes[span.start..span.end];
        db.execute(
            "INSERT INTO passages (document, start, end, line, hash) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                (start + span.start) as i64,
                (start + span.end) as i64,
                line - 1 + span.line as i64,
                semantic::hash(piece)
            ],
        )?;
        batch.words.push(Change::Passage {
            passage: db.last_insert_rowid(),
            document: id,
            text: String::from_utf8_lossy(piece).into_owned(),
        });
    }
    // The links in what it grew by, on their lines.
    let grown = &bytes[grew_at..];
    let before = bytes[..grew_at].iter().filter(|b| **b == b'\n').count() as i64;
    for link in links::parse(path, &String::from_utf8_lossy(grown)) {
        db.execute(
            "INSERT INTO links (document, target, wiki, line) VALUES (?1, ?2, ?3, ?4)",
            params![
                id,
                link.target,
                link.wiki,
                line + before + link.line as i64 - 1
            ],
        )?;
    }
    let (kind, number) = now.kept();
    db.execute(
        "UPDATE documents SET size = ?2, modified = ?3, since_kind = ?4, since = ?5, hash = ?6
         WHERE id = ?1",
        params![
            id,
            file.size,
            file.modified,
            kind,
            number,
            semantic::hash(&[&hash_before.to_le_bytes()[..], grown].concat())
        ],
    )?;
    Ok(true)
}

/// Document `id`'s links, at `path`, from its `body`.
pub(crate) fn insert_links(db: &Connection, id: i64, path: &str, body: &str) -> Result<()> {
    for link in links::parse(path, body) {
        db.execute(
            "INSERT INTO links (document, target, wiki, line) VALUES (?1, ?2, ?3, ?4)",
            params![id, link.target, link.wiki, link.line as i64],
        )?;
    }
    Ok(())
}
