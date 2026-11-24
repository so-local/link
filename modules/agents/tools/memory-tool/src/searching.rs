//! [`Reader::search`]: matches by words and by meaning, merged, each with
//! its snippet, copies dropped. By words, the passages holding every word
//! first, then those holding the rarer words, until a deadline.

use super::*;

/// A search's answer, a page of it: its hits, whether it stopped ranking
/// at its deadline, and whether more may match than it ranks.
#[derive(Debug, Default)]
pub struct Searched {
    pub hits: Vec<Hit>,
    /// Ranking stopped at the search's deadline
    /// ([`crate::opinions::Opinions::deadline`]): what's here is the best
    /// of what it ranked, and more may match.
    pub stopped: bool,
    /// More documents match than either way ranks ([`RANKED`]).
    pub beyond: bool,
}

/// What a search by words found: each document by its best passage
/// (its id, its passage, the passage's line, how it ranks), those holding
/// every word first; whether it stopped at its deadline; and whether it
/// found more documents than it ranks.
struct ByWords {
    ranked: Vec<(i64, i64, usize, Ranked)>,
    stopped: bool,
    beyond: bool,
}

impl Reader {
    /// The best `limit` matches for `query` among the documents `filter`
    /// lets through (a kind, or none: every document): by its words and,
    /// when on, by its meaning, merged. A document ranks by its best
    /// passage, whose first line is the hit's `line`.
    pub fn search(&self, query: &str, filter: impl Into<Filter>, limit: usize) -> Result<Vec<Hit>> {
        Ok(self.search_from(query, filter, 0, limit)?.hits)
    }

    /// [`Reader::search`], from the `from`th of what it ranks (a hit's
    /// [`Hit::at`] carries on after it): those before are passed over
    /// unread, but for their passages' hashes, so a copy of one is never
    /// shown again.
    pub fn search_from(
        &self,
        query: &str,
        filter: impl Into<Filter>,
        from: usize,
        limit: usize,
    ) -> Result<Searched> {
        let filter = filter.into();
        let terms = self.common.words.terms(query);
        if terms.is_empty() {
            bail!("search for some words");
        }
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(
                self.common
                    .deadline_ms
                    .load(std::sync::atomic::Ordering::Relaxed),
            );
        let words = self.by_words(&terms, &filter, deadline)?;
        let mut meaning = self.by_meaning(query, &filter).unwrap_or_default();
        by_recency(&mut meaning, |m| &m.3);
        let word_ids: Vec<i64> = words.ranked.iter().map(|w| w.0).collect();
        let meaning_ids: Vec<i64> = meaning.iter().map(|m| m.0).collect();
        let mut searched = Searched {
            stopped: words.stopped,
            beyond: words.beyond || meaning.len() >= RANKED,
            ..Searched::default()
        };
        let hits = &mut searched.hits;
        // Each kept hit's passages, to know a copy of it, and which of
        // them are shown.
        let mut kept: Vec<(Passages, Option<usize>)> = Vec::new();
        let fused = semantic::fuse(&word_ids, &meaning_ids);
        for (at, (id, _, by_words, by_meaning)) in fused.into_iter().enumerate() {
            if hits.len() == limit {
                break;
            }
            let found = words
                .ranked
                .iter()
                .find(|w| w.0 == id)
                .or_else(|| meaning.iter().find(|m| m.0 == id));
            let Some((_, passage, line, _)) = found else {
                continue;
            };
            // A copy of one kept: named on it, not shown again.
            let these = self.passages_of(id)?;
            if let Some((_, shown)) = kept.iter().find(|k| copies(&k.0, &these)) {
                if let Some(i) = shown {
                    let path: String = self.db.query_row(
                        "SELECT path FROM documents WHERE id = ?1",
                        [id],
                        |row| row.get(0),
                    )?;
                    hits[*i].copies.push(path);
                }
                continue;
            }
            if at < from {
                kept.push((these, None));
                continue;
            }
            let (path, kind, title, start, end): (String, String, String, i64, i64) =
                self.db.query_row(
                    "SELECT d.path, d.kind, d.title, p.start, p.end
                     FROM documents d JOIN passages p ON p.document = d.id
                     WHERE d.id = ?1 AND p.id = ?2",
                    [id, *passage],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )?;
            // Its passage, read from its file: one gone since shows its
            // title alone.
            let bytes = crate::source::range(
                &self.db,
                self.common.files.as_ref(),
                id,
                &path,
                start as u64,
                end as u64,
            )?
            .unwrap_or_default();
            let text = String::from_utf8_lossy(&bytes);
            let (snippet, line) = match by_words {
                true => (
                    snippet_in(&text, query).unwrap_or_else(|| excerpt(&text)),
                    line + first_line_with(&text, query) - 1,
                ),
                false => (excerpt(&text), *line),
            };
            kept.push((these, Some(hits.len())));
            hits.push(Hit {
                path,
                kind: kind_of(&kind),
                title,
                snippet,
                matched: match (by_words, by_meaning) {
                    (true, true) => "both",
                    (true, false) => "words",
                    _ => "meaning",
                },
                line,
                copies: Vec::new(),
                at: at + 1,
            });
        }
        Ok(searched)
    }

    /// The documents holding `terms`, best first, each by its best
    /// passage ([`ByWords`]), ranked by Tantivy's BM25
    /// ([`crate::index::words`]). Those whose passages hold every word come
    /// first. When too few do, those holding any of the rarer words follow:
    /// a word in more than `rank-common` of the passages (`limits.toml`) is
    /// left out when the query has rarer ones. A word of the document's
    /// title adds to its best passage's score, and a document only its
    /// title matches ranks with the second. Ranking stops at `deadline`
    /// with what it has.
    fn by_words(
        &self,
        terms: &[String],
        filter: &Filter,
        deadline: std::time::Instant,
    ) -> Result<ByWords> {
        let words = &self.common.words;
        let searcher = words.searcher();
        let limits = crate::limits::limits();
        let allowed = self.allowed(filter)?;
        let lets = |document: i64| allowed.as_ref().is_none_or(|a| a.contains(&document));
        // Every word.
        let every = words.best(&searcher, terms, true, &lets, RANKED, deadline)?;
        let scored = |best: &crate::index::words::Best| {
            self.common
                .scored
                .fetch_add(best.scored, std::sync::atomic::Ordering::Relaxed);
        };
        scored(&every);
        let mut stopped = every.stopped;
        let mut beyond = every.beyond;
        let found_every: HashSet<i64> = every.found.iter().map(|f| f.document).collect();
        // Then the rarer words, while too few hold every one.
        let mut holding = Vec::with_capacity(terms.len());
        for term in terms {
            holding.push(words.holding(&searcher, term)?);
        }
        let all = self.passages()?;
        let common = |holding: u64| holding as f64 > all as f64 * limits.rank_common;
        // A word no passage holds is no rarer word to rank by.
        let rarer = holding.iter().any(|n| *n > 0 && !common(*n));
        let mut any = Vec::new();
        if found_every.len() < RANKED && terms.len() > 1 && !stopped {
            let asked: Vec<String> = terms
                .iter()
                .zip(&holding)
                .filter(|(_, n)| !rarer || !common(**n))
                .map(|(term, _)| term.clone())
                .collect();
            let unfound = |document: i64| lets(document) && !found_every.contains(&document);
            let found = words.best(&searcher, &asked, false, &unfound, RANKED, deadline)?;
            scored(&found);
            stopped |= found.stopped;
            beyond |= found.beyond;
            any = found.found;
        }
        // Each title holding a word adds the rarity of those it holds,
        // summed, times the title's weight; a document no passage of which
        // holds one ranks by its title alone, from its first passage.
        let mut rarity: HashMap<&str, f64> = HashMap::new();
        for (term, n) in terms.iter().zip(&holding) {
            rarity.insert(term, words.rarity(&searcher, *n)?);
        }
        let weigh_title = |title: &str| {
            limits.rank_title
                * words
                    .in_title(title, terms)
                    .iter()
                    .map(|t| rarity.get(t.as_str()).copied().unwrap_or(0.0))
                    .sum::<f64>()
        };
        if !stopped {
            let found: HashSet<i64> = found_every
                .iter()
                .copied()
                .chain(any.iter().map(|f| f.document))
                .collect();
            let untitled = |document: i64| lets(document) && !found.contains(&document);
            let titled = words.titled(&searcher, terms, &untitled, RANKED, deadline)?;
            scored(&titled);
            stopped |= titled.stopped;
            let mut first = self.db.prepare_cached(
                "SELECT id FROM passages WHERE document = ?1 ORDER BY start LIMIT 1",
            )?;
            for found in titled.found {
                let passage: Option<i64> = first
                    .query_row([found.document], |row| row.get(0))
                    .optional()?;
                if let Some(passage) = passage {
                    any.push(crate::index::words::Found {
                        passage,
                        score: 0.0,
                        ..found
                    });
                }
            }
        }
        // Each group, each document by its best passage, best first.
        let mut of = self.db.prepare_cached(
            "SELECT p.line, d.modified, d.path, d.title
             FROM passages p JOIN documents d ON d.id = p.document
             WHERE p.id = ?1 AND d.id = ?2",
        )?;
        let mut group = |found: Vec<crate::index::words::Found>| -> Result<Vec<_>> {
            let mut ranked = Vec::with_capacity(found.len());
            for found in found {
                let row: Option<(i64, i64, String, String)> = of
                    .query_row([found.passage, found.document], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                    })
                    .optional()?;
                // Gone since Tantivy's searcher was taken.
                let Some((line, modified, path, title)) = row else {
                    continue;
                };
                let ranked_as = Ranked {
                    score: found.score + weigh_title(&title),
                    modified,
                    path,
                };
                ranked.push((
                    found.document,
                    found.passage,
                    line.max(1) as usize,
                    ranked_as,
                ));
            }
            ranked.sort_by(|a, b| b.3.score.total_cmp(&a.3.score).then(a.0.cmp(&b.0)));
            ranked.truncate(RANKED);
            by_recency(&mut ranked, |w| &w.3);
            Ok(ranked)
        };
        let mut ranked = group(every.found)?;
        let rest = group(any)?;
        ranked.extend(rest);
        beyond |= ranked.len() > RANKED;
        ranked.truncate(RANKED);
        Ok(ByWords {
            ranked,
            stopped,
            beyond,
        })
    }

    /// The documents `filter` lets through, by their IDs; none when it
    /// lets every document through.
    fn allowed(&self, filter: &Filter) -> Result<Option<HashSet<i64>>> {
        if *filter == Filter::default() {
            return Ok(None);
        }
        let mut statement = self.db.prepare_cached(&format!(
            "SELECT d.id FROM documents d WHERE {}",
            Filter::SQL
        ))?;
        let ids = statement
            .query_map(
                named_params! {":kind": filter.kind(), ":under": filter.under(),
                ":since": filter.since()},
                |row| row.get(0),
            )?
            .collect::<rusqlite::Result<HashSet<i64>>>()?;
        Ok(Some(ids))
    }

    /// How many passages the index holds, as counted while they're
    /// written.
    fn passages(&self) -> Result<u64> {
        Ok(self
            .db
            .query_row("SELECT value FROM meta WHERE key = 'passages'", [], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?
            .unwrap_or(0)
            .max(0) as u64)
    }

    /// Document `id`'s passages' hashes, to know a copy of it.
    fn passages_of(&self, id: i64) -> Result<Passages> {
        let mut statement = self
            .db
            .prepare_cached("SELECT hash FROM passages WHERE document = ?1")?;
        let each = statement
            .query_map([id], |row| row.get(0))?
            .collect::<rusqlite::Result<HashSet<i64>>>()?;
        Ok(Passages { each })
    }

    /// The documents nearest `query` in meaning, best first, each by its
    /// nearest passage (its id and line); none when meaning is off or
    /// the model can't be reached. Every passage's code is compared with
    /// the query's, from the mapped codes' file, and the best few hundred
    /// rescored with their small vectors ([`crate::vectors`]): no vector is
    /// loaded whole.
    fn by_meaning(&self, query: &str, filter: &Filter) -> Result<Vec<(i64, i64, usize, Ranked)>> {
        let State::On(embedder) = self.common.semantic.state() else {
            return Ok(Vec::new());
        };
        let Some(kept) = crate::vectors::Kept::held(&self.db, &self.common.file, embedder.model())?
        else {
            return Ok(Vec::new());
        };
        // Embedded through this call's own reader: a model slow to answer
        // holds up no other call.
        let asked = kept.reduce(&semantic::normalized(embedder.embed_query(query)?));
        let slots = crate::vectors::slots(&self.db)?;
        let nearest = kept.scan(slots, &asked, crate::limits::limits().rescore)?;
        let mut passage_of = self.db.prepare_cached(&format!(
            "SELECT p.id, p.document, p.line, d.modified, d.path
             FROM passages p JOIN documents d ON d.id = p.document
             WHERE p.slot = :slot AND {}",
            Filter::SQL
        ))?;
        // Each document's nearest passage, and its score.
        let mut best: HashMap<i64, (i64, usize, f64, i64, String)> = HashMap::new();
        for (_, slot) in nearest {
            let found: Option<(i64, i64, i64, i64, String)> = passage_of
                .query_row(
                    named_params! {":slot": slot as i64, ":kind": filter.kind(),
                    ":under": filter.under(), ":since": filter.since()},
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
            let Some((passage, document, line, modified, path)) = found else {
                continue;
            };
            self.common.small_read.fetch_add(
                kept.small_read() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
            let Some(vector) = kept.read_small(slot, passage)? else {
                continue;
            };
            let score = f64::from(semantic::dot(&asked, &vector));
            // Unrelated, however it ranks.
            if score <= 0.0 {
                continue;
            }
            if best.get(&document).is_none_or(|b| score > b.2) {
                best.insert(document, (passage, line as usize, score, modified, path));
            }
        }
        let mut ranked: Vec<(i64, i64, usize, Ranked)> = best
            .into_iter()
            .map(|(document, (passage, line, score, modified, path))| {
                let ranked = Ranked {
                    score,
                    modified,
                    path,
                };
                (document, passage, line, ranked)
            })
            .collect();
        ranked.sort_by(|a, b| b.3.score.total_cmp(&a.3.score).then(a.0.cmp(&b.0)));
        ranked.truncate(RANKED);
        Ok(ranked)
    }
}

/// The first line of `body` holding one of `query`'s words, from 1.
fn first_line_with(body: &str, query: &str) -> usize {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(str::to_lowercase)
        .collect();
    body.lines()
        .position(|line| {
            let line = line.to_lowercase();
            words.iter().any(|w| line.contains(w.as_str()))
        })
        .map_or(1, |n| n + 1)
}

/// About 16 words around the first word of `query` in `body`, each
/// word of it in bold, as SQLite's `snippet` shows them; none when no
/// word is there as written (SQLite also finds its stems).
fn snippet_in(body: &str, query: &str) -> Option<String> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let matches = |token: &str| {
        let token = token.to_lowercase();
        words.iter().any(|w| token.contains(w.as_str()))
    };
    body.lines().find_map(|line| {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let first = tokens.iter().position(|t| matches(t))?;
        let start = first.saturating_sub(4);
        let end = (start + 16).min(tokens.len());
        let shown: Vec<String> = tokens[start..end]
            .iter()
            .map(|t| {
                if matches(t) {
                    format!("**{t}**")
                } else {
                    t.to_string()
                }
            })
            .collect();
        let before = if start > 0 { "…" } else { "" };
        let after = if end < tokens.len() { "…" } else { "" };
        Some(format!("{before}{}{after}", shown.join(" ")))
    })
}

/// A passage's start, for a hit found by meaning, or by words none of
/// which is in it as written (SQLite also finds their stems).
fn excerpt(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(200) {
        Some((at, _)) => format!("{}…", &flat[..at]),
        None => flat,
    }
}

/// How well a document matched, for ranking: its score (higher is
/// better), when it last changed, its path.
struct Ranked {
    score: f64,
    modified: i64,
    path: String,
}

/// `items` best first, and among those that match about as well (each
/// within [`AS_WELL`] of the best of them), the newest first; between
/// equals, the shorter path (the original, not `... copy`).
fn by_recency<T>(items: &mut [T], ranked: impl Fn(&T) -> &Ranked) {
    items.sort_by(|a, b| {
        let (a, b) = (ranked(a), ranked(b));
        b.score.total_cmp(&a.score)
    });
    let mut start = 0;
    while start < items.len() {
        let best = ranked(&items[start]).score;
        let end = start
            + items[start..]
                .iter()
                .take_while(|i| ranked(i).score >= best - best.abs() * AS_WELL)
                .count();
        items[start..end].sort_by(|a, b| {
            let (a, b) = (ranked(a), ranked(b));
            b.modified
                .cmp(&a.modified)
                .then(a.path.len().cmp(&b.path.len()))
                .then(a.path.cmp(&b.path))
        });
        start = end;
    }
}

/// A document's passages, as [`copies`] compares them: each one's hash.
struct Passages {
    each: HashSet<i64>,
}

/// Whether two documents' passages make them copies: all the same or,
/// with many, nearly all.
fn copies(a: &Passages, b: &Passages) -> bool {
    if a.each == b.each {
        return true;
    }
    if a.each.len().min(b.each.len()) < NEAR_PASSAGES {
        return false;
    }
    let shared = a.each.intersection(&b.each).count();
    let union = a.each.len() + b.each.len() - shared;
    shared as f64 / union as f64 >= NEAR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_documents_snippet_is_around_its_first_match() {
        let body = format!(
            "{}\nwe renewed the Passport at the office on the corner of Main and Elm \
             after a long wait in line with everyone else\n",
            "nothing here\n".repeat(10)
        );
        assert_eq!(
            snippet_in(&body, "passport").unwrap(),
            "we renewed the **Passport** at the office on the corner of Main and Elm after a…"
        );
        assert_eq!(snippet_in(&body, "visa"), None);
    }

    #[test]
    fn copies_are_the_same_passages_or_many_and_nearly() {
        let of = |hashes: std::ops::Range<i64>| Passages {
            each: hashes.collect(),
        };
        assert!(copies(&of(0..3), &of(0..3)));
        // Short: only the same passages.
        assert!(!copies(&of(0..3), &of(0..4)));
        // Many: nearly all.
        assert!(copies(&of(0..40), &of(0..41)));
        assert!(!copies(&of(0..40), &of(10..50)));
    }
}
