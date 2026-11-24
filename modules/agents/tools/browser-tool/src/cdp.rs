//! Chromium's DevTools protocol over a pipe (`--remote-debugging-pipe`):
//! a JSON message each way, each ended by a NUL byte. A thread reads the
//! browser's messages, handing each answer to the call waiting on it and
//! keeping the events its caller reads for whoever asks next, at most
//! [`Keep::bytes`] of them for each page; an event its caller answers at
//! once (a page's dialog, which holds the page until answered) it answers
//! as it comes. An event of a frame or tab the caller doesn't watch
//! ([`Cdp::watch`]), as a page's own frames are, is dropped as it comes.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

/// A connection to a browser.
pub struct Cdp {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    next: Arc<AtomicU64>,
    shared: Arc<Shared>,
}

/// An event's answer, made by the reader as the event comes: a call of
/// `method` with `params` on the event's page.
pub type Answer = (&'static str, Value);

/// How a connection answers the events it answers at once.
pub type Answering = Box<dyn Fn(&Value) -> Option<Answer> + Send>;

/// Which events a connection keeps unread, and how it answers those it
/// answers at once.
pub struct Keep {
    /// The methods of the events kept; any other is dropped as it comes.
    pub methods: Vec<String>,
    /// The most bytes of one page's events kept unread: past it, its
    /// oldest go, and an event larger is dropped as it comes.
    pub bytes: usize,
    /// The most bytes one message may hold: a larger one ends the
    /// connection.
    pub message: usize,
    /// The answer an event gets as it comes, if any. The event is kept
    /// too, with the answer's params beside it (`answered`).
    pub answer: Answering,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Default)]
struct State {
    waiting: HashMap<u64, Sender<Value>>,
    /// Each event kept, its page ([`page_of`]), and its size as it came.
    events: VecDeque<Kept>,
    /// The bytes of the events kept, by page.
    bytes: HashMap<String, usize>,
    /// The tabs (targets) whose frames' and openings' events are kept.
    watched: HashSet<String>,
    /// Why the browser's side ended, once it has.
    ended: Option<String>,
}

impl Cdp {
    /// A connection writing to `writer` and reading `reader`, on a thread
    /// of its own until `reader` ends, keeping the events `keep` names.
    pub fn new(
        writer: impl Write + Send + 'static,
        reader: impl Read + Send + 'static,
        keep: Keep,
    ) -> Cdp {
        let cdp = Cdp {
            writer: Arc::new(Mutex::new(Box::new(writer))),
            next: Arc::new(AtomicU64::new(1)),
            shared: Arc::new(Shared::default()),
        };
        let reading = Cdp {
            writer: cdp.writer.clone(),
            next: cdp.next.clone(),
            shared: cdp.shared.clone(),
        };
        std::thread::spawn(move || read(BufReader::new(reader), &reading, &keep));
        cdp
    }

    /// Sends `method` with `params` on page `session`, its answer unread.
    fn send(&self, method: &str, params: Value, session: Option<&str>) -> Result<()> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.write(id, method, params, session)
    }

    /// Writes call `id` of `method` with `params` on page `session`.
    fn write(&self, id: u64, method: &str, params: Value, session: Option<&str>) -> Result<()> {
        let mut message = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        let mut bytes = serde_json::to_vec(&message)?;
        bytes.push(0);
        writer
            .write_all(&bytes)
            .and_then(|_| writer.flush())
            .context("writing to the browser")
    }

    /// Calls `method` with `params`, on the page `session` attached when
    /// given, and waits at most `timeout` for its result.
    pub fn call(
        &self,
        method: &str,
        params: Value,
        session: Option<&str>,
        timeout: Duration,
    ) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        {
            let mut state = self.shared.lock();
            if let Some(why) = &state.ended {
                bail!("the browser ended: {why}");
            }
            state.waiting.insert(id, tx);
        }
        if let Err(err) = self.write(id, method, params, session) {
            self.shared.lock().waiting.remove(&id);
            return Err(err);
        }
        let answer = rx.recv_timeout(timeout);
        self.shared.lock().waiting.remove(&id);
        let answer = match answer {
            Ok(answer) => answer,
            Err(_) => match &self.shared.lock().ended {
                Some(why) => bail!("the browser ended: {why}"),
                None => bail!(
                    "the browser didn't answer {method} within {}s: if it stays stuck, \
                     'close' it and open the page again",
                    timeout.as_secs()
                ),
            },
        };
        if let Some(error) = answer.get("error") {
            let message = error.get("message").and_then(Value::as_str).unwrap_or("");
            bail!("{method}: {message}");
        }
        Ok(answer.get("result").cloned().unwrap_or(Value::Null))
    }

    /// The first event `wanted` takes, taken from those kept, waiting
    /// until `deadline` for one; the others stay.
    pub fn event(&self, deadline: Instant, wanted: impl Fn(&Value) -> bool) -> Option<Value> {
        let mut state = self.shared.lock();
        loop {
            if let Some(at) = state.events.iter().position(|k| wanted(&k.event)) {
                return state.remove(at);
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            if state.ended.is_some() {
                return None;
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Every event `wanted` takes, taken now, without waiting.
    pub fn take(&self, wanted: impl Fn(&Value) -> bool) -> Vec<Value> {
        let mut state = self.shared.lock();
        let (taken, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut state.events)
            .into_iter()
            .partition(|k| wanted(&k.event));
        state.bytes.clear();
        for k in &kept {
            *state.bytes.entry(k.page.clone()).or_default() += k.size;
        }
        state.events = kept.into();
        taken.into_iter().map(|k| k.event).collect()
    }

    /// Keeps the events of tab `target`'s own frame and of the tabs it
    /// opens from now on, until [`Cdp::unwatch`].
    pub fn watch(&self, target: &str) {
        self.shared.lock().watched.insert(target.to_string());
    }

    /// Drops tab `target`'s events from now on, and those kept.
    pub fn unwatch(&self, target: &str) {
        let mut state = self.shared.lock();
        state.watched.remove(target);
        drop(state);
        self.take(|e| page_of(e) == target && e.get("sessionId").is_none());
    }

    /// Whether the browser's side has ended.
    pub fn ended(&self) -> bool {
        self.shared.lock().ended.is_some()
    }
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// An event kept: it, its page, and its size as it came.
struct Kept {
    event: Value,
    page: String,
    size: usize,
}

/// Whose page an event is: its session, or the tab (target) its frame
/// or opener is, or the browser's own (`""`).
fn page_of(event: &Value) -> &str {
    event["sessionId"]
        .as_str()
        .or(event["params"]["frameId"].as_str())
        .or(event["params"]["targetInfo"]["openerId"].as_str())
        .unwrap_or_default()
}

impl State {
    /// Takes the event at `at`.
    fn remove(&mut self, at: usize) -> Option<Value> {
        let kept = self.events.remove(at)?;
        if let Some(bytes) = self.bytes.get_mut(&kept.page) {
            *bytes -= kept.size;
        }
        Some(kept.event)
    }

    /// Whether `event` is of a tab watched, as an event naming a frame
    /// or an opener must be: a page's own frames' loads, and tabs no
    /// page opened, are never kept.
    fn watches(&self, event: &Value) -> bool {
        let params = &event["params"];
        [&params["frameId"], &params["targetInfo"]["openerId"]]
            .into_iter()
            .filter_map(Value::as_str)
            .all(|target| self.watched.contains(target))
            && (event["method"] != "Target.targetCreated"
                || params["targetInfo"]["openerId"].is_string())
    }

    /// Keeps `event`, of `size` bytes, dropping its page's oldest past
    /// `most`; an event larger than `most` is dropped.
    fn keep(&mut self, event: Value, size: usize, most: usize) {
        if size > most {
            return;
        }
        let page = page_of(&event).to_string();
        let bytes = self.bytes.entry(page.clone()).or_default();
        *bytes += size;
        let mut over = *bytes > most;
        self.events.push_back(Kept {
            event,
            page: page.clone(),
            size,
        });
        while over {
            let Some(at) = self.events.iter().position(|k| k.page == page) else {
                break;
            };
            self.remove(at);
            over = self.bytes.get(&page).is_some_and(|b| *b > most);
        }
    }
}

/// Reads the browser's messages until it ends, each answer to its
/// caller, each event `keep` names kept, and answered when it says.
fn read(mut reader: impl BufRead, cdp: &Cdp, keep: &Keep) {
    let shared = &*cdp.shared;
    let why = loop {
        let mut bytes = Vec::new();
        match reader
            .by_ref()
            .take(keep.message as u64 + 1)
            .read_until(0, &mut bytes)
        {
            Ok(0) => break "it closed its pipe".to_string(),
            Err(err) => break format!("reading it: {err}"),
            Ok(_) => {}
        }
        if bytes.last() != Some(&0) {
            break match bytes.len() > keep.message {
                true => format!("it sent a message past {} bytes", keep.message),
                false => "it closed its pipe".to_string(),
            };
        }
        bytes.pop();
        let Ok(mut message) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if let Some(id) = message.get("id").and_then(Value::as_u64) {
            if let Some(tx) = shared.lock().waiting.remove(&id) {
                let _ = tx.send(message);
            }
            continue;
        }
        let method = message["method"].as_str().unwrap_or_default();
        if !keep.methods.iter().any(|m| m == method) || !shared.lock().watches(&message) {
            continue;
        }
        if let Some((call, params)) = (keep.answer)(&message) {
            let session = message["sessionId"].as_str().map(str::to_string);
            // Its answer is never read: a page gone meanwhile only fails.
            let _ = cdp.send(call, params.clone(), session.as_deref());
            message["answered"] = params;
        }
        let mut state = shared.lock();
        state.keep(message, bytes.len(), keep.bytes);
        shared.changed.notify_all();
    };
    let mut state = shared.lock();
    state.ended = Some(why);
    // Each caller waiting hears now, not at its timeout.
    state.waiting.clear();
    shared.changed.notify_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps loads and dialogs, a dialog answered OK.
    fn keep(bytes: usize) -> Keep {
        Keep {
            methods: ["Page.loadEventFired", "Page.javascriptDialogOpening"]
                .map(String::from)
                .into(),
            bytes,
            message: 1 << 20,
            answer: Box::new(|e| {
                (e["method"] == "Page.javascriptDialogOpening")
                    .then(|| ("Page.handleJavaScriptDialog", json!({"accept": true})))
            }),
        }
    }

    /// Events the caller doesn't read are dropped as they come, a dialog
    /// is answered before anyone asks, and past the byte cap the oldest
    /// events go.
    #[test]
    fn only_events_read_are_kept_and_a_dialog_is_answered_as_it_comes() {
        let (to_browser_r, to_browser_w) = std::io::pipe().unwrap();
        let (from_browser_r, mut from_browser_w) = std::io::pipe().unwrap();
        let (heard, calls) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(to_browser_r);
            loop {
                let mut bytes = Vec::new();
                if reader.read_until(0, &mut bytes).unwrap_or(0) == 0 {
                    return;
                }
                bytes.pop();
                let call: Value = serde_json::from_slice(&bytes).unwrap();
                if call["method"] == "Page.enable" {
                    for event in [
                        r#"{"method":"Network.dataReceived","params":{}}"#,
                        r#"{"method":"Page.loadEventFired","sessionId":"s2","params":{"n":0}}"#,
                        r#"{"method":"Page.javascriptDialogOpening","sessionId":"s1","params":{"type":"confirm"}}"#,
                        r#"{"method":"Page.loadEventFired","sessionId":"s1","params":{"n":1}}"#,
                        r#"{"method":"Page.loadEventFired","sessionId":"s1","params":{"n":2,"pad":"..................................................................................................................................................."}}"#,
                        r#"{"method":"Page.loadEventFired","sessionId":"s1","params":{"n":3}}"#,
                    ] {
                        from_browser_w
                            .write_all(format!("{event}\0").as_bytes())
                            .unwrap();
                    }
                }
                let answer = json!({"id": call["id"], "result": {}});
                from_browser_w
                    .write_all(format!("{answer}\0").as_bytes())
                    .unwrap();
                let _ = heard.send(call);
            }
        });
        // Room for two of a page's loads, not the dialog before them; the
        // padded load, past the cap alone, is dropped as it comes; and
        // another page's load is never pushed out by this one's.
        let cdp = Cdp::new(to_browser_w, from_browser_r, keep(150));
        let second = Duration::from_secs(5);
        cdp.call("Page.enable", json!({}), Some("s1"), second)
            .unwrap();
        let soon = Instant::now() + second;
        let third_load = cdp.event(soon, |e| e["params"]["n"] == 3);
        assert!(third_load.is_some());
        let other_page = cdp.take(|e| e["sessionId"] == "s2");
        assert_eq!(other_page.len(), 1, "{other_page:?}");
        let answered = std::iter::from_fn(|| calls.recv_timeout(second).ok())
            .find(|call| call["method"] == "Page.handleJavaScriptDialog")
            .unwrap();
        assert_eq!(answered["sessionId"], "s1");
        assert_eq!(answered["params"]["accept"], true);
        let left = cdp.take(|_| true);
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0]["params"]["n"], 1);
    }

    /// A page's own frames' loads, a tab no watched page opened, and a
    /// tab the browser opened by itself are never kept; the watched
    /// tab's frame and the tabs it opens are.
    #[test]
    fn only_watched_tabs_frames_and_openings_are_kept() {
        let mut state = State::default();
        state.watched.insert("T1".into());
        let frame = |id: &str| json!({"method": "Page.frameStartedLoading", "sessionId": "s1", "params": {"frameId": id}});
        let opened = |by: Value| json!({"method": "Target.targetCreated", "params": {"targetInfo": {"type": "page", "openerId": by}}});
        assert!(state.watches(&frame("T1")));
        assert!(!state.watches(&frame("subframe")));
        assert!(state.watches(&opened(json!("T1"))));
        assert!(!state.watches(&opened(json!("T2"))));
        assert!(!state.watches(&opened(Value::Null)));
        assert!(
            state.watches(
                &json!({"method": "Page.loadEventFired", "sessionId": "s1", "params": {}})
            )
        );
    }

    /// A browser that answers each call with its method and sends an
    /// event before the second answer.
    #[test]
    fn answers_go_to_their_callers_and_events_are_kept() {
        let (to_browser_r, to_browser_w) = std::io::pipe().unwrap();
        let (from_browser_r, mut from_browser_w) = std::io::pipe().unwrap();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(to_browser_r);
            for n in 0.. {
                let mut bytes = Vec::new();
                if reader.read_until(0, &mut bytes).unwrap_or(0) == 0 {
                    return;
                }
                bytes.pop();
                let call: Value = serde_json::from_slice(&bytes).unwrap();
                if n == 1 {
                    from_browser_w
                        .write_all(b"{\"method\":\"Page.loadEventFired\",\"params\":{}}\0")
                        .unwrap();
                }
                let answer = json!({"id": call["id"], "result": {"method": call["method"]}});
                from_browser_w
                    .write_all(format!("{answer}\0").as_bytes())
                    .unwrap();
            }
        });
        let cdp = Cdp::new(to_browser_w, from_browser_r, keep(1 << 20));
        let second = Duration::from_secs(5);
        assert_eq!(
            cdp.call("Browser.getVersion", json!({}), None, second)
                .unwrap()["method"],
            "Browser.getVersion"
        );
        cdp.call("Page.enable", json!({}), Some("s1"), second)
            .unwrap();
        let soon = Instant::now() + second;
        let loaded = cdp.event(soon, |e| e["method"] == "Page.loadEventFired");
        assert!(loaded.is_some());
        assert!(cdp.event(Instant::now(), |_| true).is_none());
    }

    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_browser_gone_ends_each_call_at_once() {
        let (_unread, to_browser_w) = std::io::pipe().unwrap();
        let (from_browser_r, from_browser_w) = std::io::pipe().unwrap();
        let cdp = Cdp::new(to_browser_w, from_browser_r, keep(1 << 20));
        drop(from_browser_w);
        let started = Instant::now();
        while !cdp.ended() {
            std::thread::sleep(Duration::from_millis(5));
        }
        let err = cdp
            .call(
                "Browser.getVersion",
                json!({}),
                None,
                Duration::from_secs(30),
            )
            .unwrap_err();
        assert!(err.to_string().contains("ended"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
