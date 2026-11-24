//! A child's output, shown as it comes and logged a line at a time: the
//! one writer of an agent's log when its harness runs in a terminal.

use std::io::{Read, Write};

use super::{Entry, Ids, Level, Line, Shown};
use crate::render::Ctx;

/// The longest line waited for: longer, it's taken in pieces, so a
/// program printing without newlines can't make a reader hold ever more.
pub const LONGEST_LINE: usize = 64 << 10;

/// Bytes cut into lines as they come ([`Split::push`]).
#[derive(Default)]
pub struct Split {
    partial: Vec<u8>,
}

impl Split {
    /// The lines `bytes` ends, with what came before them: each whole
    /// (ended by a newline, which is left off, or [`LONGEST_LINE`] long).
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.partial.extend_from_slice(bytes);
        let mut lines = Vec::new();
        // Walked by an offset and taken out once: linear in what's read.
        let mut at = 0;
        loop {
            let rest = &self.partial[at..];
            let (end, next) = match rest[..rest.len().min(LONGEST_LINE)]
                .iter()
                .position(|b| *b == b'\n')
            {
                Some(end) => (end, end + 1),
                None if rest.len() >= LONGEST_LINE => (LONGEST_LINE, LONGEST_LINE),
                None => break,
            };
            lines.push(text(&rest[..end]));
            at += next;
        }
        self.partial.drain(..at);
        lines
    }

    /// What's left, a line with no end: at the end of what's read.
    pub fn rest(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.partial);
        (!rest.is_empty()).then(|| text(&rest))
    }

    /// Whether a line has begun and not ended.
    fn begun(&self) -> bool {
        !self.partial.is_empty()
    }

    /// Whether the line begun opens as a record does.
    fn record_begun(&self) -> bool {
        self.partial.first() == Some(&b'{')
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\r')
        .to_string()
}

/// Copies `from`, a child's output, until it ends: shown on `shown` as
/// it comes, styled as `ctx` says, and to `log` a [`Line`] a line. A line
/// that is a record (the child writes its log, as the Link Harness does)
/// is logged as it is, naming `agent` where it names none, and shown
/// rendered; any other is shown as it came and logged as a record of its
/// own, with its time and `agent`. A failure to show or to log stops
/// neither.
pub fn relay(
    mut from: impl Read,
    shown: &mut impl Write,
    ctx: &Ctx,
    log: &mut impl Write,
    agent: Option<&str>,
) {
    let mut split = Split::default();
    let mut buffer = [0u8; 8192];
    // Whether the line being read is held back from `shown` until its
    // end, since it may be a record, which shows rendered.
    let mut held = false;
    while let Ok(n) = from.read(&mut buffer) {
        if n == 0 {
            break;
        }
        let mut bytes = &buffer[..n];
        while !bytes.is_empty() {
            if !split.begun() {
                held = bytes[0] == b'{';
            }
            let end = bytes
                .iter()
                .position(|b| *b == b'\n')
                .map_or(bytes.len(), |end| end + 1);
            let (piece, rest) = bytes.split_at(end);
            if !held {
                let _ = shown.write_all(piece).and_then(|()| shown.flush());
            }
            for line in split.push(piece) {
                logged(&line, held, shown, ctx, log, agent);
                held = split.record_begun();
            }
            bytes = rest;
        }
    }
    if let Some(line) = split.rest() {
        logged(&line, held, shown, ctx, log, agent);
        if !held {
            let _ = shown.write_all(b"\n");
        }
    }
    let _ = shown.flush();
}

/// `line` logged, and shown if it was `held` back.
fn logged(
    line: &str,
    held: bool,
    shown: &mut impl Write,
    ctx: &Ctx,
    log: &mut impl Write,
    agent: Option<&str>,
) {
    let record = Line::parse(line);
    if held {
        let text = match &record {
            Some(record) => Shown {
                entry: Entry::Line(record.clone()),
                times: 1,
            }
            .render(agent, ctx),
            None => line.to_string(),
        };
        let _ = shown
            .write_all(format!("{text}\n").as_bytes())
            .and_then(|()| shown.flush());
    }
    let mut record = record.unwrap_or_else(|| Line::now(Level::Info, line, Ids::default()));
    if record.agent.is_none() {
        record.agent = agent.map(str::to_string);
    }
    let _ = record.write(log);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_output_is_shown_as_it_came_and_logged_as_records() {
        let mut said = Vec::new();
        Line::now(
            Level::Warning,
            "tool web left out",
            Ids::default().chat("link:1"),
        )
        .write(&mut said)
        .unwrap();
        let output = [b"Listening on 8080\n".as_slice(), &said, b"no end"].concat();
        let (mut shown, mut log) = (Vec::new(), Vec::new());
        relay(
            output.as_slice(),
            &mut shown,
            &Ctx::plain(),
            &mut log,
            Some("a1"),
        );
        let shown = String::from_utf8(shown).unwrap();
        let lines: Vec<&str> = shown.lines().collect();
        assert_eq!(lines[0], "Listening on 8080");
        assert!(
            lines[1].ends_with("! tool web left out  chat link:1"),
            "{shown}"
        );
        assert_eq!(lines[2], "no end");
        let log = String::from_utf8(log).unwrap();
        let records: Vec<Line> = log.lines().map(|l| Line::parse(l).unwrap()).collect();
        assert_eq!(
            records.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            ["Listening on 8080", "tool web left out", "no end"]
        );
        assert!(records.iter().all(|r| r.agent.as_deref() == Some("a1")));
        assert_eq!(records[1].chat.as_deref(), Some("link:1"));
    }

    #[test]
    fn a_hostile_line_shows_no_escapes() {
        // Escapes that write the clipboard, link text elsewhere, move the
        // cursor or rewrite the line; a C1 CSI; bytes that aren't UTF-8.
        let hostile = "a\x1b]52;c;ZXZpbA==\x07b \x1b]8;;https://evil.example\x1b\\here\x1b]8;;\x1b\\ \
                       \x1b[2J\x1b[1;1Hc\rd\u{9b}31m\x08e\tf\n";
        let mut bytes = hostile.as_bytes().to_vec();
        bytes.extend_from_slice(b"\xff\xfe g\nno end yet");
        let mut split = Split::default();
        let entries: Vec<Entry> = split.push(&bytes).iter().map(|l| Entry::read(l)).collect();
        assert_eq!(
            entries,
            [
                Entry::Said {
                    text:
                        "a]52;c;ZXZpbA==b ]8;;https://evil.example\\here]8;;\\ [2J[1;1Hcd31me    f"
                            .into()
                },
                Entry::Said {
                    text: "\u{fffd}\u{fffd} g".into()
                },
            ]
        );
        assert_eq!(split.rest().as_deref(), Some("no end yet"));
    }

    #[test]
    fn a_burst_of_lines_is_split_in_one_pass() {
        let burst = format!("{}\n", "x".repeat(79)).repeat(64 << 10);
        let mut split = Split::default();
        assert_eq!(split.push(burst.as_bytes()).len(), 64 << 10);
        assert_eq!(split.rest(), None);
    }

    #[test]
    fn a_line_is_taken_in_pieces_past_the_longest() {
        let mut split = Split::default();
        assert!(split.push(b"one\ntw").len() == 1);
        assert_eq!(split.push(b"o\n"), ["two"]);
        let long = vec![b'x'; LONGEST_LINE + 3];
        let lines = split.push(&long);
        assert_eq!(lines.len(), 1);
        assert_eq!(split.rest().as_deref(), Some("xxx"));
    }
}
