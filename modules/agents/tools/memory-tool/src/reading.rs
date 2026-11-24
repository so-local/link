//! A document read by its lines ([`Reader::read`]), its links, and the
//! documents listed.

use super::*;

impl Reader {
    /// Document `path` (as a search or list gives it), read from its file
    /// ([`crate::source`]), or a skill's file (`skills/...`), read by its
    /// path though never indexed ([`tree::Files`]): its lines from `from` (the first
    /// is 1), `lines` of them or all, and no more than `tokens` of them.
    /// Whole lines, at least one: a longer line is cut at `tokens`, and
    /// says so (`cut`). Where it stopped early, `next`.
    pub fn read(
        &self,
        path: &str,
        from: usize,
        lines: Option<usize>,
        tokens: Option<usize>,
    ) -> Result<Option<Document>> {
        let asked = path.trim_start_matches("./");
        let path = asked.trim_start_matches('/');
        let found = self
            .db
            .query_row(
                "SELECT id, kind, title FROM documents WHERE path = ?1",
                [path],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((id, kind, title)) = found {
            // Read, so its passages are embedded sooner.
            self.common.semantic.was_read(id);
            let most = crate::limits::limits().source_max_bytes.max(
                self.common
                    .file_most
                    .load(std::sync::atomic::Ordering::Relaxed),
            );
            // Gone since it was indexed: nothing there now.
            let Some(body) =
                crate::source::whole(&self.db, self.common.files.as_ref(), id, path, most)?
            else {
                return Ok(None);
            };
            return Ok(Some(slice(
                path,
                kind_of(&kind),
                title,
                &body,
                from,
                lines,
                tokens,
            )));
        }
        // A skill's file, by its own path only: never one asked for from
        // the root (`/skills/...`).
        if asked.starts_with('/') || !tree::unindexed(path) {
            return Ok(None);
        }
        let most = crate::limits::limits().file_max_bytes;
        let Some(bytes) = self.common.files.read(path, most)? else {
            return Ok(None);
        };
        let body = String::from_utf8_lossy(&bytes);
        let title = title(path, Kind::File, &body, refreshing::Titled::ByHeading);
        Ok(Some(slice(
            path,
            Kind::File,
            title,
            &bytes,
            from,
            lines,
            tokens,
        )))
    }

    /// Document `path`'s links: what it links to, what links to it, and
    /// what it links to that memory doesn't hold. A link names a
    /// document by its path, and a wiki link by its path or its file's
    /// name (the shortest path, when several share it).
    pub fn links(&self, path: &str) -> Result<Option<Links>> {
        let path = path.trim_start_matches("./").trim_start_matches('/');
        let Some((id, kind, title)) = self
            .db
            .query_row(
                "SELECT id, kind, title FROM documents WHERE path = ?1",
                [path],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get(2)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let written: Vec<(String, bool, usize)> = self
            .db
            .prepare("SELECT target, wiki, line FROM links WHERE document = ?1 ORDER BY line")?
            .query_map([id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as usize))
            })?
            .collect::<rusqlite::Result<_>>()?;
        // What links here: by its path, or by a name that finds it.
        let [lower, without, file] = links::names_of(path);
        let pointing: Vec<(String, bool, String, String, String, usize)> = self
            .db
            .prepare(
                "SELECT l.target, l.wiki, d.path, d.kind, d.title, l.line
                 FROM links l JOIN documents d ON d.id = l.document
                 WHERE d.id != :id AND ((l.wiki = 0 AND l.target = :path)
                     OR (l.wiki = 1 AND l.target IN (:lower, :without, :file)))
                 ORDER BY d.path, l.line",
            )?
            .query_map(
                named_params! {":id": id, ":path": path, ":lower": lower,
                ":without": without, ":file": file},
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get::<_, i64>(5)? as usize,
                    ))
                },
            )?
            .collect::<rusqlite::Result<_>>()?;
        // Every wiki name either way, found in one read of the paths.
        let names: HashSet<&str> = written
            .iter()
            .filter(|link| link.1)
            .map(|link| link.0.as_str())
            .chain(
                pointing
                    .iter()
                    .filter(|link| link.1)
                    .map(|link| link.0.as_str()),
            )
            .collect();
        let named = self.wiki(&names)?;
        // The first of each shown, found and missing.
        let (mut to, mut missing) = (Vec::new(), Vec::new());
        for (target, is_wiki, line) in written {
            if to.len() >= crate::limits::limits().links_max
                && missing.len() >= crate::limits::limits().links_max
            {
                break;
            }
            let found = match is_wiki {
                true => named.get(&target).cloned(),
                false => self.document(&target)?,
            };
            match found {
                Some((path, kind, title)) => to.push(Linked {
                    path,
                    kind: kind_of(&kind),
                    title,
                    line,
                }),
                None => missing.push(Missing { target, line }),
            }
        }
        let mut from = Vec::new();
        for (target, is_wiki, other, kind, title, line) in pointing {
            if from.len() >= crate::limits::limits().links_max {
                break;
            }
            // A name another document of that name finds first isn't here.
            if is_wiki && named.get(&target).is_none_or(|(p, _, _)| p != path) {
                continue;
            }
            from.push(Linked {
                path: other,
                kind: kind_of(&kind),
                title,
                line,
            });
        }
        to.truncate(crate::limits::limits().links_max);
        from.truncate(crate::limits::limits().links_max);
        missing.truncate(crate::limits::limits().links_max);
        Ok(Some(Links {
            path: path.to_string(),
            kind: kind_of(&kind),
            title,
            to,
            from,
            missing,
        }))
    }

    /// The document at `path`: its path, kind and title.
    fn document(&self, path: &str) -> Result<Option<(String, String, String)>> {
        Ok(self
            .db
            .query_row(
                "SELECT path, kind, title FROM documents WHERE path = ?1",
                [path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?)
    }

    /// The document each wiki name of `names` finds: of those it names,
    /// the shortest path. One read of every path, however many names.
    fn wiki(&self, names: &HashSet<&str>) -> Result<HashMap<String, (String, String, String)>> {
        let mut found: HashMap<String, (String, String, String)> = HashMap::new();
        if names.is_empty() {
            return Ok(found);
        }
        let mut statement = self.db.prepare("SELECT path, kind, title FROM documents")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let path = row.get_ref(0)?.as_str()?;
            for name in links::names_of(path) {
                if !names.contains(name.as_str())
                    || found
                        .get(&name)
                        .is_some_and(|(p, _, _)| (p.len(), p.as_str()) <= (path.len(), path))
                {
                    continue;
                }
                found.insert(name, (path.to_string(), row.get(1)?, row.get(2)?));
            }
        }
        Ok(found)
    }

    /// The `limit` documents changed most recently that `filter` lets
    /// through, and how many it lets through in all. With `since`, each
    /// says whether it's new or changed, and what was removed after it
    /// comes too.
    pub fn list(&self, filter: &Filter, limit: usize) -> Result<Listing> {
        self.list_after(filter, None, limit)
    }

    /// [`Reader::list`], after the document `after` names (when it
    /// changed and its path, as a page names it), read from the index's
    /// order there; what was removed comes only with the first page.
    pub fn list_after(
        &self,
        filter: &Filter,
        after: Option<(i64, &str)>,
        limit: usize,
    ) -> Result<Listing> {
        let (kind, under, since) = (filter.kind(), filter.under(), filter.since());
        let total: i64 = self.db.query_row(
            &format!("SELECT count(*) FROM documents d WHERE {}", Filter::SQL),
            named_params! {":kind": kind, ":under": under, ":since": since},
            |row| row.get(0),
        )?;
        let mut statement = self.db.prepare(&format!(
            "SELECT path, kind, title, size, modified, added, changed FROM documents d
             INDEXED BY documents_changed
             WHERE {} AND (:after IS NULL OR d.changed < :after
                 OR (d.changed = :after AND d.path > :path))
             ORDER BY d.changed DESC, d.path LIMIT :limit",
            Filter::SQL
        ))?;
        let (after, path) = after.unzip();
        let mut documents: Vec<Entry> = statement
            .query_map(
                named_params! {":kind": kind, ":under": under, ":since": since,
                ":after": after, ":path": path, ":limit": limit.saturating_add(1) as i64},
                |row| {
                    let added: i64 = row.get(5)?;
                    Ok(Entry {
                        changed: row.get(6)?,
                        path: row.get(0)?,
                        kind: kind_of(&row.get::<_, String>(1)?),
                        title: row.get(2)?,
                        size: row.get::<_, i64>(3)? as u64,
                        modified: row.get::<_, i64>(4)? / 1_000_000_000,
                        change: since.map(|since| match added > since {
                            true => "new",
                            false => "changed",
                        }),
                    })
                },
            )?
            .collect::<rusqlite::Result<_>>()?;
        let more = documents.len() > limit;
        documents.truncate(limit);
        let removed = match since.filter(|_| after.is_none()) {
            None => None,
            Some(since) => {
                let mut statement = self.db.prepare(
                    "SELECT path, kind, title, at FROM removed d
                     WHERE at > :since AND (:kind IS NULL OR kind = :kind)
                     AND (:under IS NULL OR path LIKE :under ESCAPE '\\')
                     ORDER BY at DESC, path LIMIT :limit",
                )?;
                let removed = statement
                    .query_map(
                        named_params! {":since": since, ":kind": kind, ":under": under,
                        ":limit": limit as i64},
                        |row| {
                            Ok(Removed {
                                path: row.get(0)?,
                                kind: kind_of(&row.get::<_, String>(1)?),
                                title: row.get(2)?,
                                removed: row.get::<_, i64>(3)? / 1_000_000_000,
                            })
                        },
                    )?
                    .collect::<rusqlite::Result<_>>()?;
                Some(removed)
            }
        };
        Ok(Listing {
            documents,
            total: total as usize,
            removed,
            more,
        })
    }
}

/// `body`'s lines from `from` (the first is 1), `lines` of them or all,
/// and no more than `tokens` of them, as [`Reader::read`] gives them.
fn slice(
    path: &str,
    kind: Kind,
    title: String,
    source: &[u8],
    from: usize,
    lines: Option<usize>,
    tokens: Option<usize>,
) -> Document {
    use link_files::reading::{ByteSpan, ContentDigest, ReadReceipt, ReadSnapshot, TextTransform};
    let body = String::from_utf8_lossy(source);
    let all: Vec<&str> = body.lines().collect();
    let start = from.saturating_sub(1).min(all.len());
    let mut end = lines.map_or(all.len(), |n| start.saturating_add(n).min(all.len()));
    let mut text = all[start..end].join("\n");
    let mut cut = false;
    if let Some(budget) = tokens.map(|t| t.max(1))
        && crate::tokens(&text) > budget
    {
        // Whole lines while they fit, at least one.
        let (mut used, mut fit) = (0, start);
        for line in &all[start..end] {
            let cost = crate::tokens(line) + 1;
            if fit > start && used + cost > budget {
                break;
            }
            used += cost;
            fit += 1;
        }
        end = fit;
        text = all[start..end].join("\n");
        if crate::tokens(&text) > budget {
            text = text.chars().take(budget * 4).collect();
            cut = true;
        }
    }
    let wanted = lines.map_or(all.len(), |n| start.saturating_add(n).min(all.len()));
    let source_start = line_offset(source, start);
    let source_end = match cut {
        true => source_start + prefix_bytes(&source[source_start..], text.chars().count()),
        false => line_offset(source, end),
    };
    let mut transformations = vec![TextTransform::Lines];
    if matches!(body, std::borrow::Cow::Owned(_)) {
        transformations.push(TextTransform::LossyUtf8);
    }
    let read = ReadReceipt {
        path: path.into(),
        source: ReadSnapshot::Whole {
            content: ContentDigest::of(source),
        },
        span: ByteSpan {
            start: source_start as u64,
            end: source_end as u64,
        },
        text: ContentDigest::of(text.as_bytes()),
        text_prefix_bytes: 0,
        transformations,
    };
    Document {
        path: path.to_string(),
        kind,
        title,
        lines: all.len(),
        text,
        read,
        next: (end < wanted).then_some(end + 1),
        cut,
    }
}

fn line_offset(source: &[u8], lines: usize) -> usize {
    source
        .split_inclusive(|byte| *byte == b'\n')
        .take(lines)
        .map(<[u8]>::len)
        .sum()
}

/// Source bytes for a prefix of lossy-decoded characters, before line
/// normalization. A clipped read always clips within its first line.
fn prefix_bytes(source: &[u8], characters: usize) -> usize {
    let (mut bytes, mut chars) = (0, 0);
    for chunk in source.utf8_chunks() {
        for (index, _) in chunk.valid().char_indices() {
            if chars == characters {
                return bytes + index;
            }
            chars += 1;
        }
        bytes += chunk.valid().len();
        if !chunk.invalid().is_empty() {
            if chars == characters {
                return bytes;
            }
            chars += 1;
            bytes += chunk.invalid().len();
        }
    }
    bytes
}

#[cfg(test)]
mod receipts;

/// A time as `since` takes it, in seconds since 1970: a number of them
/// (as `list` gives `modified`), a date (`2026-09-01`, the start of that
/// day) or a date and time (`2026-09-01T14:30:00Z`), in UTC.
pub fn parse_since(since: &str) -> Result<i64> {
    let since = since.trim();
    if let Ok(seconds) = since.parse::<i64>() {
        return Ok(seconds);
    }
    let bad = || {
        anyhow::anyhow!(
            "{since:?} isn't a date: '2026-09-01', '2026-09-01T14:30:00Z' or seconds since 1970"
        )
    };
    let (date, time) = match since.split_once(['T', ' ']) {
        Some((date, time)) => (date, time.trim_end_matches('Z')),
        None => (since, ""),
    };
    let number = |part: Option<&str>| part.and_then(|p| p.parse::<i64>().ok()).ok_or_else(bad);
    let mut day = date.splitn(3, '-');
    let (year, month, dom) = (
        number(day.next())?,
        number(day.next())?,
        number(day.next())?,
    );
    if !(1..=12).contains(&month) || !(1..=31).contains(&dom) {
        return Err(bad());
    }
    let mut clock = time.split(':').filter(|p| !p.is_empty());
    let hour = clock.next().map_or(Ok(0), |p| number(Some(p)))?;
    let minute = clock.next().map_or(Ok(0), |p| number(Some(p)))?;
    let second = clock
        .next()
        .map_or(Ok(0), |p| number(Some(p.split('.').next().unwrap_or(p))))?;
    if hour > 23 || minute > 59 || second > 60 {
        return Err(bad());
    }
    // Howard Hinnant's days-from-civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + dom - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Ok(days * 86_400 + hour * 3600 + minute * 60 + second)
}

pub(super) fn kind_of(kind: &str) -> Kind {
    Kind::parse(kind).unwrap_or(Kind::File)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_takes_seconds_a_date_or_a_date_and_time() {
        assert_eq!(parse_since("1790690587").unwrap(), 1_790_690_587);
        assert_eq!(parse_since("1970-01-01").unwrap(), 0);
        assert_eq!(parse_since("2000-02-29").unwrap(), 951_782_400);
        assert_eq!(parse_since("2026-09-29T14:03:07Z").unwrap(), 1_790_690_587);
        assert_eq!(parse_since("2026-09-29 14:03").unwrap(), 1_790_690_580);
        for bad in ["yesterday", "2026-13-01", "2026-09", "2026-09-01T25:00", ""] {
            assert!(parse_since(bad).is_err(), "{bad}");
        }
    }
}
