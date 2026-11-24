//! `lnk agent log show --live`: what your agent does as it does it, a line a
//! step: a message come, the model thinking, a tool called and answered,
//! the answer sent, and the agent stopping and starting. So a slow answer
//! shows where it waits: on the model, on a tool, or on nothing at all.
//!
//! It reads Link Harness's conversations as they're written, each one's
//! `events.jsonl` in the files folder's `chats/`, a line of JSON
//! per event, only ever added to (`docs/architecture/configuration.md`),
//! as Link Memory does; and the agent's service, as `show` does.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use link_agent::service;
use link_agent::settings::{self, Settings};
use link_format::{Done, Name, Note, Out, Report, cmd, sentence};
use serde_json::Value;

/// The longest a message's text is shown, in characters.
const SHOWN: usize = 72;

/// How long the model may think before it's said again that it still is.
const STILL: Duration = Duration::from_secs(60);

/// `lnk agent log show --live`: until Ctrl-C.
#[expect(clippy::disallowed_methods, reason = "the live view's own period")]
pub fn run(out: &mut Out, settings: &Settings) -> Result<Done, Report> {
    let model = settings
        .model
        .as_ref()
        .map(|model| match &model.name {
            Some(name) => Ok(Some(name.clone())),
            None => settings
                .spec()
                .map(|spec| Settings::model_name(model, &spec)),
        })
        .transpose()?
        .flatten()
        .map(|m| link_format::clean(&m))
        .unwrap_or_else(|| "its model".into());
    let harness = settings.harness().to_string();
    let agent = Name::new("agent", &settings.name);
    let mut running = service::running_anywhere(&settings.name);
    out.block(Note(sentence!(
        "{} live: its messages, its model and tools, its starts and stops, until Ctrl-C.",
        &agent
    )));
    if harness != settings::DEFAULT_HARNESS {
        out.block(Note(sentence!(
            "{} keeps its conversations its own way, so only its starts and stops show here.",
            Name::new("harness", &harness)
        )));
        out.block(Note(sentence!(
            "{} shows its log.",
            cmd!("lnk agent log show --follow")
        )));
    }
    out.block(Note(match running {
        true => sentence!("{} is running.", &agent),
        false => sentence!("{} is stopped.", &agent),
    }));
    let mut watch = Watch::new(
        settings
            .folder()
            .of(link_store_contract::storage::Collection::Conversations),
    );
    let mut chats: BTreeMap<String, Chat> = BTreeMap::new();
    let mut checked = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        for (chat, line) in watch.poll() {
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let state = chats.entry(chat.clone()).or_default();
            for said in state.describe(&event, &model, Instant::now()) {
                print_line(out, "chat", &chat, &said);
            }
        }
        for (chat, state) in chats.iter_mut() {
            if let Some(said) = state.still(&model, Instant::now()) {
                print_line(out, "chat", chat, &said);
            }
        }
        if checked.elapsed() >= Duration::from_secs(2) {
            checked = Instant::now();
            let now = service::running_anywhere(&settings.name);
            if now != running {
                running = now;
                let said = match now {
                    true => "started",
                    false => "stopped: lnk agent show says more",
                };
                print_line(out, "agent", &settings.name, said);
            }
        }
    }
}

fn print_line(out: &mut Out, kind: &'static str, who: &str, said: &str) {
    out.live()
        .line(Some(&Name::new(kind, who)), format!("{}  {said}", clock()));
}

/// One conversation as it's watched: what it waits on, since when.
#[derive(Default)]
struct Chat {
    /// When the message it's answering came.
    asked: Option<Instant>,
    /// Since when the model has been asked and hasn't answered.
    thinking: Option<Instant>,
    /// How many times it's been said that it still thinks.
    told: u32,
}

impl Chat {
    /// What `event` says happened, a line each.
    fn describe(&mut self, event: &Value, model: &str, now: Instant) -> Vec<String> {
        let text = |key: &str| event.get(key).and_then(Value::as_str).unwrap_or_default();
        let mut lines = Vec::new();
        match text("role") {
            "user" => {
                let from = match text("from") {
                    "" => String::new(),
                    from => format!(" from {}", link_format::clean(from)),
                };
                lines.push(format!("message{from}: {}", shown(text("text"))));
                self.asked.get_or_insert(now);
                self.think(model, now, &mut lines);
            }
            "assistant" => {
                self.thinking = None;
                let thought = event
                    .get("latency_ms")
                    .and_then(Value::as_u64)
                    .map(|ms| format!(" (thought {})", seconds(ms)))
                    .unwrap_or_default();
                let calls: Vec<&str> = event
                    .get("calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c.get("name").and_then(Value::as_str))
                    .collect();
                if !calls.is_empty() {
                    lines.push(format!(
                        "calls {}{thought}",
                        link_format::clean(&calls.join(", "))
                    ));
                } else if event.get("interrupted").and_then(Value::as_bool) == Some(true) {
                    lines.push(format!("cut short{thought}"));
                    self.asked = None;
                } else if !text("text").trim().is_empty() {
                    let tokens = event
                        .get("tokens")
                        .map(|t| {
                            let n = |k: &str| t.get(k).and_then(Value::as_u64).unwrap_or(0);
                            format!(", {} in, {} out", n("input"), n("output"))
                        })
                        .unwrap_or_default();
                    let since = self
                        .asked
                        .take()
                        .map(|at| format!(", {} after the message", secs(now - at)))
                        .unwrap_or_default();
                    lines.push(format!(
                        "answered: {}{thought}{since}{tokens}",
                        shown(text("text"))
                    ));
                }
            }
            "tool" => {
                let tool = match text("tool") {
                    "" => "a tool".to_string(),
                    tool => link_format::clean(tool),
                };
                match event.get("failed").and_then(Value::as_bool) == Some(true) {
                    true => lines.push(format!("{tool} failed: {}", shown(text("text")))),
                    false => lines.push(format!("{tool} answered")),
                }
                self.think(model, now, &mut lines);
            }
            "summary" => lines.push("summarized what left its window".into()),
            _ => {}
        }
        lines
    }

    /// The model is asked now.
    fn think(&mut self, model: &str, now: Instant, lines: &mut Vec<String>) {
        if self.thinking.is_none() {
            self.thinking = Some(now);
            self.told = 0;
            lines.push(format!("thinking on {model}..."));
        }
    }

    /// Once a minute while the model thinks: that it still does.
    fn still(&mut self, model: &str, now: Instant) -> Option<String> {
        let since = now - self.thinking?;
        let due = STILL * (self.told + 1);
        if since < due {
            return None;
        }
        self.told += 1;
        Some(format!("still thinking on {model} ({})", secs(since)))
    }
}

/// `text` on one line, quoted, cut at [`SHOWN`] characters.
fn shown(text: &str) -> String {
    let one: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let cut: String = one.chars().take(SHOWN).collect();
    match cut.len() < one.len() {
        true => format!("\"{cut}...\""),
        false => format!("\"{cut}\""),
    }
}

/// `840 ms`, `2.3 s`, `41 s`.
fn seconds(ms: u64) -> String {
    match ms {
        ms if ms < 1000 => format!("{ms} ms"),
        ms if ms < 10_000 => format!("{:.1} s", ms as f64 / 1000.0),
        ms => format!("{} s", ms / 1000),
    }
}

/// `41 s`, `2 min 5 s`.
fn secs(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        s if s >= 60 => format!("{} min {} s", s / 60, s % 60),
        s => format!("{s} s"),
    }
}

/// This machine's time of day, `10:23:34`.
fn clock() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    // SAFETY: `tm` is written by localtime_r, which reads only `now`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&now, &mut tm).is_null() };
    match ok {
        true => format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec),
        false => "--:--:--".into(),
    }
}

/// The conversations' logs, read as they grow: what's there when it
/// starts is skipped, a conversation begun since is read whole.
struct Watch {
    dir: PathBuf,
    /// Each log, by its conversation, with how far it's read and the
    /// start of a line not yet ended.
    logs: BTreeMap<String, (u64, Vec<u8>)>,
}

impl Watch {
    fn new(dir: PathBuf) -> Self {
        let mut watch = Watch {
            dir,
            logs: BTreeMap::new(),
        };
        for (chat, path) in watch.found() {
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            watch.logs.insert(chat, (len, Vec::new()));
        }
        watch
    }

    /// Each conversation's log, by its folder's name.
    fn found(&self) -> Vec<(String, PathBuf)> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path().join("events.jsonl");
                let chat = entry.file_name().to_string_lossy().into_owned();
                path.is_file().then_some((chat, path))
            })
            .collect()
    }

    /// The lines added since the last poll, each with its conversation.
    fn poll(&mut self) -> Vec<(String, String)> {
        let mut lines = Vec::new();
        for (chat, path) in self.found() {
            let (at, partial) = self.logs.entry(chat.clone()).or_default();
            for line in read_from(&path, at, partial) {
                lines.push((chat.clone(), line));
            }
        }
        lines
    }
}

/// The whole lines of `path` past `at`, moving `at` on; a line not yet
/// ended waits in `partial`.
fn read_from(path: &Path, at: &mut u64, partial: &mut Vec<u8>) -> Vec<String> {
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if len < *at {
        // Made anew: read from its start.
        *at = 0;
        partial.clear();
    }
    if len == *at {
        return Vec::new();
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    if file.seek(SeekFrom::Start(*at)).is_err() {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if file.take(len - *at).read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    *at = len;
    partial.extend(bytes);
    let mut lines = Vec::new();
    while let Some(end) = partial.iter().position(|b| *b == b'\n') {
        let line: Vec<u8> = partial.drain(..=end).collect();
        let line = String::from_utf8_lossy(&line).trim().to_string();
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `lines` without their colors.
    fn plain(lines: Vec<String>) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                let mut out = String::new();
                let mut chars = line.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '\u{1b}' => {
                            for c in chars.by_ref() {
                                if c == 'm' {
                                    break;
                                }
                            }
                        }
                        c => out.push(c),
                    }
                }
                out
            })
            .collect()
    }

    #[test]
    fn a_turn_reads_as_its_steps() {
        let mut chat = Chat::default();
        let start = Instant::now();
        let model = "qwen3:8b";
        assert_eq!(
            plain(chat.describe(
                &json!({"seq": 1, "role": "user", "text": "what is my\nfavorite color?"}),
                model,
                start
            )),
            [
                "message: \"what is my favorite color?\"",
                "thinking on qwen3:8b..."
            ]
        );
        // Nothing to say before a minute; then once a minute.
        assert_eq!(chat.still(model, start + Duration::from_secs(30)), None);
        assert_eq!(
            chat.still(model, start + Duration::from_secs(61))
                .as_deref(),
            Some("still thinking on qwen3:8b (1 min 1 s)")
        );
        assert_eq!(chat.still(model, start + Duration::from_secs(90)), None);
        assert_eq!(
            plain(chat.describe(
                &json!({"role": "assistant", "text": "", "latency_ms": 23000,
                        "calls": [{"id": "1", "name": "memory__search", "arguments": {}}]}),
                model,
                start + Duration::from_secs(95)
            )),
            ["calls memory__search (thought 23 s)"]
        );
        assert_eq!(chat.still(model, start + Duration::from_secs(200)), None);
        assert_eq!(
            plain(chat.describe(
                &json!({"role": "tool", "tool": "memory__search", "text": "no note holds it",
                        "failed": true}),
                model,
                start + Duration::from_secs(96)
            )),
            [
                "memory__search failed: \"no note holds it\"",
                "thinking on qwen3:8b..."
            ]
        );
        assert_eq!(
            plain(chat.describe(
                &json!({"role": "assistant", "text": "Red.", "latency_ms": 840,
                        "tokens": {"input": 193, "output": 83, "cached": 0}}),
                model,
                start + Duration::from_secs(113)
            )),
            ["answered: \"Red.\" (thought 840 ms), 1 min 53 s after the message, 193 in, 83 out"]
        );
        assert_eq!(chat.still(model, start + Duration::from_secs(400)), None);
    }

    #[test]
    fn a_message_from_a_schedule_says_whose() {
        let mut chat = Chat::default();
        let lines = chat.describe(
            &json!({"role": "user", "text": "check the news", "from": "schedule:7"}),
            "m",
            Instant::now(),
        );
        assert_eq!(
            plain(lines)[0],
            "message from schedule:7: \"check the news\""
        );
    }

    #[test]
    fn a_long_message_is_cut() {
        let long = "a".repeat(100);
        assert_eq!(shown(&long), format!("\"{}...\"", "a".repeat(SHOWN)));
        assert_eq!(shown("hi\u{1b}[31m"), "\"hi[31m\"");
    }

    #[test]
    fn only_lines_written_since_are_read() {
        let dir = std::env::temp_dir().join(format!("lnk-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let old = dir.join("telegram-1");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("events.jsonl"), "{\"seq\":1}\n").unwrap();
        let mut watch = Watch::new(dir.clone());
        assert!(watch.poll().is_empty());
        // A line added, and half of another.
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(old.join("events.jsonl"))
            .unwrap();
        write!(file, "{{\"seq\":2}}\n{{\"seq\"").unwrap();
        assert_eq!(watch.poll(), [("telegram-1".into(), "{\"seq\":2}".into())]);
        writeln!(file, ":3}}").unwrap();
        assert_eq!(watch.poll(), [("telegram-1".into(), "{\"seq\":3}".into())]);
        // A conversation begun since: read whole.
        let new = dir.join("slack-D1");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("events.jsonl"), "{\"seq\":1}\n").unwrap();
        assert_eq!(watch.poll(), [("slack-D1".into(), "{\"seq\":1}".into())]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
