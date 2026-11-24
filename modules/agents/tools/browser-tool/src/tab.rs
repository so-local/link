//! One conversation's page: its browser context, its tab, and what's
//! done on it. An owner's opens in the profile's own context, its
//! sign-ins kept; any other's in a context made for it, which keeps
//! nothing past the browser and sees none of the profile's.
//!
//! The page is read and its parts found in a world of the tool's own
//! (`Page.createIsolatedWorld`), apart from the page's scripts, which
//! can neither see nor forge what it keeps.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::cdp::Cdp;
use crate::fencing::{PAGE, refused_download};
use crate::{Reading, by_page};

/// What the tool's world is named in each page.
const WORLD: &str = "link";

/// A page's answer to its next confirm, prompt, or leave-page question.
#[derive(Clone, Debug)]
pub(crate) struct Answer {
    pub accept: bool,
    /// What a prompt is answered with.
    pub text: Option<String>,
}

/// Each page's next answer, by its session.
pub(crate) type Answers = Arc<Mutex<HashMap<String, Answer>>>;

/// How the connection answers a page's dialog as it opens, before
/// anything waits on the page: an alert with OK, which is all it offers;
/// a confirm, prompt, or leave-page question as `dialog` last said for
/// that page, once, and otherwise dismissed.
pub(crate) fn answering(answers: Answers) -> crate::cdp::Answering {
    Box::new(move |event| {
        if event["method"] != "Page.javascriptDialogOpening" {
            return None;
        }
        let params = match event["params"]["type"].as_str() {
            Some("alert") => json!({"accept": true}),
            _ => {
                let session = event["sessionId"].as_str().unwrap_or_default();
                let answer = answers
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(session);
                match answer {
                    Some(Answer { accept, text }) => {
                        json!({"accept": accept, "promptText": text.unwrap_or_default()})
                    }
                    None => json!({"accept": false}),
                }
            }
        };
        Some(("Page.handleJavaScriptDialog", params))
    })
}

/// Refuses downloads in browser context `context` (the profile's when
/// none), each told as it's refused.
pub(crate) fn deny_downloads(cdp: &Cdp, context: Option<&str>) -> Result<()> {
    let mut params = json!({"behavior": "deny", "eventsEnabled": true});
    if let Some(context) = context {
        params["browserContextId"] = json!(context);
    }
    cdp.call(
        "Browser.setDownloadBehavior",
        params,
        None,
        crate::settings::get().call(),
    )?;
    Ok(())
}

/// A conversation's page: its context (none: the profile's), its tab,
/// the session attached to it, and the tool's world in it while the
/// page that made it stands.
pub(crate) struct Tab {
    pub context: Option<String>,
    pub target: String,
    pub session: String,
    world: Option<i64>,
    /// When a call last used it.
    pub used: Instant,
}

impl Tab {
    /// A new page: in the profile's context for an `owner`, else in a
    /// context of its own.
    pub fn new(cdp: &Cdp, owner: bool) -> Result<Tab> {
        let context = match owner {
            true => None,
            false => {
                let made = cdp.call(
                    "Target.createBrowserContext",
                    json!({}),
                    None,
                    crate::settings::get().call(),
                )?;
                let context = made["browserContextId"]
                    .as_str()
                    .context("the browser made no context")?
                    .to_string();
                deny_downloads(cdp, Some(&context))?;
                Some(context)
            }
        };
        let mut params = json!({"url": "about:blank"});
        if let Some(context) = &context {
            params["browserContextId"] = json!(context);
        }
        let target = cdp.call(
            "Target.createTarget",
            params,
            None,
            crate::settings::get().call(),
        )?["targetId"]
            .as_str()
            .context("the browser made no page")?
            .to_string();
        let mut tab = Tab {
            context,
            target: String::new(),
            session: String::new(),
            world: None,
            used: Instant::now(),
        };
        Page {
            cdp,
            tab: &mut tab,
            sandbox: "",
        }
        .attach(&target)?;
        Ok(tab)
    }

    /// Closes the page, and its context when it has one of its own.
    pub fn close(&self, cdp: &Cdp) {
        cdp.unwatch(&self.target);
        let _ = cdp.call(
            "Target.closeTarget",
            json!({"targetId": self.target}),
            None,
            crate::settings::get().call(),
        );
        if let Some(context) = &self.context {
            let _ = cdp.call(
                "Target.disposeBrowserContext",
                json!({"browserContextId": context}),
                None,
                crate::settings::get().call(),
            );
        }
    }
}

/// A conversation's page, on the browser running it, in the sandbox
/// named `sandbox`.
pub(crate) struct Page<'a> {
    pub cdp: &'a Cdp,
    pub tab: &'a mut Tab,
    pub sandbox: &'a str,
}

impl Page<'_> {
    /// Attaches to page `target`, which is the page from now on.
    fn attach(&mut self, target: &str) -> Result<()> {
        self.cdp.watch(target);
        let attached = self.cdp.call(
            "Target.attachToTarget",
            json!({"targetId": target, "flatten": true}),
            None,
            crate::settings::get().call(),
        )?;
        let session = attached["sessionId"]
            .as_str()
            .context("the browser gave no session")?
            .to_string();
        for (method, params) in [
            ("Page.enable", json!({})),
            (
                "Page.setInterceptFileChooserDialog",
                json!({"enabled": true}),
            ),
            (
                "Emulation.setDeviceMetricsOverride",
                json!({"width": crate::settings::get().width, "height": crate::settings::get().height, "deviceScaleFactor": 1, "mobile": false}),
            ),
        ] {
            self.cdp.call(
                method,
                params,
                Some(&session),
                crate::settings::get().call(),
            )?;
        }
        self.tab.target = target.to_string();
        self.tab.session = session;
        self.tab.world = None;
        Ok(())
    }

    /// Opens `url`, waits for it to load, and reads it.
    pub fn go(&mut self, url: &url::Url) -> Result<(Reading, Vec<String>)> {
        self.forget_loading();
        let navigated = self.cdp.call(
            "Page.navigate",
            json!({"url": url.as_str()}),
            Some(&self.tab.session),
            crate::settings::get().call(),
        )?;
        if let Some(error) = navigated["errorText"].as_str().filter(|e| !e.is_empty()) {
            bail!("{}", self.unreached(url.as_str(), error));
        }
        let mut notes = Vec::new();
        if navigated["loaderId"].is_string() {
            self.loaded(Instant::now() + crate::settings::get().load(), &mut notes)?;
        }
        // A browser that ended meanwhile is said, never a blank page
        // read from the one started in its place.
        if self.cdp.ended() {
            bail!("the browser ended while {url} loaded: open it again to start it anew");
        }
        self.forget_loading();
        self.notes(&mut notes)?;
        Ok((self.read()?, notes))
    }

    /// Clicks part `n` of the last read.
    pub fn click(&mut self, n: u64) -> Result<Vec<String>> {
        let (x, y) = self.place(n)?;
        self.click_at(x, y)?;
        let mut notes = vec![format!("Clicked [{n}].")];
        self.after_input(&mut notes);
        Ok(notes)
    }

    /// Types `text` into part `n`, what it held replaced, and presses
    /// Enter after with `submit`.
    pub fn type_into(&mut self, n: u64, text: &str, submit: bool) -> Result<Vec<String>> {
        let (x, y) = self.place(n)?;
        self.click_at(x, y)?;
        // Clicked, the page may have changed: a later failure is a note,
        // so the model reads before typing again.
        match self.type_after_click(n, text, submit) {
            Ok(mut notes) => {
                self.after_input(&mut notes);
                Ok(notes)
            }
            Err(err) => Ok(vec![format!(
                "Clicked [{n}] to type, but then: {err:#}. Read the page before doing it again."
            )]),
        }
    }

    /// What typing into part `n` does once it's clicked: what it held
    /// selected, `text` put in, and Enter pressed with `submit`.
    fn type_after_click(&mut self, n: u64, text: &str, submit: bool) -> Result<Vec<String>> {
        self.evaluate(&format!(
            "(() => {{ const e = (globalThis.__linkParts || [])[{n}]; if (!e) return; \
             e.focus(); if (e.isContentEditable) {{ e.ownerDocument.execCommand('selectAll'); }} \
             else if ('value' in e && e.tagName !== 'SELECT') {{ e.select ? e.select() : (e.value = ''); }} }})()"
        ))?;
        if !text.is_empty() {
            self.cdp.call(
                "Input.insertText",
                json!({"text": text}),
                Some(&self.tab.session),
                crate::settings::get().call(),
            )?;
        }
        let mut notes = vec![format!("Typed into [{n}].")];
        if submit {
            self.key("Enter")?;
            notes.push("Pressed Enter.".into());
        }
        Ok(notes)
    }

    /// Presses key `name` where the page's focus is.
    pub fn press(&mut self, name: &str) -> Result<Vec<String>> {
        self.forget_loading();
        self.key(name)?;
        let mut notes = vec![format!("Pressed {name}.")];
        self.after_input(&mut notes);
        Ok(notes)
    }

    /// Goes back to the page before this one.
    pub fn back(&mut self) -> Result<Vec<String>> {
        let history = self.cdp.call(
            "Page.getNavigationHistory",
            json!({}),
            Some(&self.tab.session),
            crate::settings::get().call(),
        )?;
        let at = history["currentIndex"].as_u64().unwrap_or(0) as usize;
        let Some(before) = at
            .checked_sub(1)
            .and_then(|i| history["entries"].get(i))
            .and_then(|entry| entry["id"].as_i64())
        else {
            bail!("there's no page before this one");
        };
        self.forget_loading();
        self.cdp.call(
            "Page.navigateToHistoryEntry",
            json!({"entryId": before}),
            Some(&self.tab.session),
            crate::settings::get().call(),
        )?;
        let mut notes = vec!["Went back.".to_string()];
        self.after_input(&mut notes);
        Ok(notes)
    }

    /// Moves the pointer over part `n`.
    pub fn hover(&mut self, n: u64) -> Result<Vec<String>> {
        let (x, y) = self.place(n)?;
        self.cdp.call(
            "Input.dispatchMouseEvent",
            json!({"type": "mouseMoved", "x": x, "y": y}),
            Some(&self.tab.session),
            crate::settings::get().call(),
        )?;
        let mut notes = vec![format!("The pointer is over [{n}].")];
        self.notes(&mut notes)?;
        Ok(notes)
    }

    /// Chooses `values` (each an option's text or value) in choice `n`:
    /// the one value of a choice of one, each value of a choice of many,
    /// its other options unchosen, as Playwright's `selectOption` does.
    pub fn select(&mut self, n: u64, values: &[String]) -> Result<Vec<String>> {
        self.forget_loading();
        let chosen = self.evaluate(&format!(
            "(() => {{ const e = (globalThis.__linkParts || [])[{n}]; \
             if (!e || !e.isConnected) return null; \
             if (e.tagName !== 'SELECT') return {{error: 'not a choice'}}; \
             const want = {}; const norm = (t) => t.replace(/\\s+/g, ' ').trim().toLowerCase(); \
             if (!e.multiple && want.length !== 1) return {{one: true}}; \
             const found = want.map((w) => [...e.options].find((o) => o.value === w) || \
               [...e.options].find((o) => norm(o.text) === norm(w))); \
             const missing = want.filter((w, i) => !found[i]); \
             if (missing.length) return {{error: 'no such option', missing, options: [...e.options].map((o) => o.text.trim())}}; \
             for (const o of e.options) o.selected = found.includes(o); \
             e.dispatchEvent(new Event('input', {{bubbles: true}})); \
             e.dispatchEvent(new Event('change', {{bubbles: true}})); \
             return {{chose: found.map((o) => o.text.trim())}}; }})()",
            serde_json::to_string(values)?
        ))?;
        if chosen.is_null() {
            bail!("{}", gone(n));
        }
        if chosen["one"] == true {
            bail!(
                "[{n}] is a choice of one: give one value, not {}",
                values.len()
            );
        }
        if let Some(error) = chosen["error"].as_str() {
            match chosen["options"].as_array() {
                Some(options) => {
                    let texts = |list: &Value| -> Vec<String> {
                        list.as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect()
                    };
                    let missing = texts(&chosen["missing"]);
                    let options: Vec<&str> = options.iter().filter_map(Value::as_str).collect();
                    bail!(
                        "[{n}] has no option {missing:?}: it has these\n{}",
                        by_page(link_untrusted::Kind::Page, PAGE, &options.join(", "))?
                    )
                }
                None => bail!("[{n}] is {error}: 'click' or 'type' it"),
            }
        }
        let chose: Vec<&str> = chosen["chose"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let mut notes = vec![format!(
            "Chose this in [{n}]:\n{}",
            by_page(link_untrusted::Kind::Page, PAGE, &chose.join(", "))?
        )];
        self.after_input(&mut notes);
        Ok(notes)
    }

    /// Scrolls part `n` into view, or the page `down` or up by most of a
    /// window. Where it is then.
    #[expect(
        clippy::disallowed_methods,
        reason = "the page scrolls once drawn, which nothing announces"
    )]
    pub fn scroll(&mut self, n: Option<u64>, down: bool) -> Result<Vec<String>> {
        match n {
            Some(n) => {
                self.place(n)?;
            }
            None => {
                let settings = crate::settings::get();
                let by = (settings.height as f64 * settings.scroll_share)
                    * if down { 1.0 } else { -1.0 };
                self.cdp.call(
                    "Input.dispatchMouseEvent",
                    json!({"type": "mouseWheel", "x": crate::settings::get().width / 2, "y": crate::settings::get().height / 2,
                        "deltaX": 0, "deltaY": by}),
                    Some(&self.tab.session),
                    crate::settings::get().call(),
                )?;
                // The wheel scrolls once the page has drawn.
                std::thread::sleep(settings.scroll());
            }
        }
        let at = self.evaluate(
            "({y: Math.round(scrollY), height: document.documentElement.scrollHeight, \
             window: innerHeight})",
        )?;
        let (y, height, window) = (
            at["y"].as_u64().unwrap_or(0),
            at["height"].as_u64().unwrap_or(0),
            at["window"].as_u64().unwrap_or(0),
        );
        let place = match (y, y + window >= height) {
            (0, true) => "The whole page is in view.".to_string(),
            (0, false) => "At the page's top.".to_string(),
            (_, true) => "At the page's end.".to_string(),
            (y, false) => format!("{y} of {height} pixels down."),
        };
        let mut notes = vec![place];
        self.notes(&mut notes)?;
        Ok(notes)
    }

    /// What the window shows, as a PNG.
    pub fn screenshot(&mut self) -> Result<(Vec<u8>, Vec<String>)> {
        let shot = self.cdp.call(
            "Page.captureScreenshot",
            json!({"format": "png"}),
            Some(&self.tab.session),
            crate::settings::get().call(),
        )?;
        let data = shot["data"].as_str().context("no screenshot came")?;
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD
            .decode(data)
            .context("the screenshot")?;
        let mut notes = Vec::new();
        self.notes(&mut notes)?;
        Ok((png, notes))
    }

    /// After input was sent: what came of it, and a failure while
    /// waiting on it said as a note, never as the call's error, so the
    /// input isn't sent again.
    fn after_input(&mut self, notes: &mut Vec<String>) {
        if let Err(err) = self.settle(notes) {
            notes.push(format!(
                "It was sent, but then: {err:#}. Read the page before doing it again."
            ));
        }
    }

    /// After an action: a page it started loading waited for, and what
    /// else came of it said.
    fn settle(&mut self, notes: &mut Vec<String>) -> Result<()> {
        let session = self.tab.session.clone();
        let target = self.tab.target.clone();
        // The page's own frame starting to load, never one of its frames.
        let started = self
            .cdp
            .event(Instant::now() + crate::settings::get().settle(), |e| {
                e["method"] == "Page.frameStartedLoading"
                    && e["sessionId"] == session.as_str()
                    && e["params"]["frameId"] == target.as_str()
            });
        if started.is_some() {
            self.loaded(Instant::now() + crate::settings::get().load(), notes)?;
        }
        if self.cdp.ended() {
            bail!("the browser ended");
        }
        // A page it opened in a new tab is the page now, the old tab
        // closed; of several, the last, every other closed unread.
        let opened: Vec<String> = self
            .cdp
            .take(|e| {
                e["method"] == "Target.targetCreated"
                    && e["params"]["targetInfo"]["type"] == "page"
                    && e["params"]["targetInfo"]["openerId"] == target.as_str()
            })
            .iter()
            .filter_map(|e| e["params"]["targetInfo"]["targetId"].as_str())
            .map(str::to_string)
            .collect();
        if let Some((tab, others)) = opened.split_last() {
            for other in others {
                self.close_target(other);
            }
            self.attach(tab)?;
            self.close_target(&target);
            notes.push(match others.len() {
                0 => "It opened a new tab, which is the page now.".into(),
                n => format!(
                    "It opened {} new tabs: the last is the page now, the others closed.",
                    n + 1
                ),
            });
            // One that loaded before it was attached sends no load
            // event: its state is asked instead, a blank page still on
            // its way to the address it was opened with.
            let ready = self.evaluate(
                "document.readyState === 'complete' && location.href !== 'about:blank'",
            )?;
            if ready != true {
                self.loaded(Instant::now() + crate::settings::get().load(), notes)?;
            }
        }
        self.notes(notes)?;
        let at = self.evaluate("({title: document.title, url: location.href})")?;
        // The address and the title are the page's to set: fenced.
        let url = at["url"].as_str().unwrap_or("");
        let (url, _) = crate::capped(url, crate::settings::get().note_bytes);
        notes.push(format!(
            "Now at this page, its title:\n{}",
            by_page(
                link_untrusted::Kind::Dialog,
                url,
                at["title"].as_str().unwrap_or("")
            )?
        ));
        Ok(())
    }

    /// Closes tab `target`, its events no longer kept.
    fn close_target(&self, target: &str) {
        self.cdp.unwatch(target);
        let _ = self.cdp.call(
            "Target.closeTarget",
            json!({"targetId": target}),
            None,
            crate::settings::get().call(),
        );
    }

    /// Waits until the page has loaded or stopped loading, or
    /// `deadline`, its dialogs told meanwhile.
    fn loaded(&self, deadline: Instant, notes: &mut Vec<String>) -> Result<()> {
        let session = self.tab.session.clone();
        while Instant::now() < deadline && !self.cdp.ended() {
            self.dialogs(notes)?;
            let soon = (Instant::now() + crate::settings::get().load_poll()).min(deadline);
            let event = self.cdp.event(soon, |e| {
                e["sessionId"] == session.as_str()
                    && (e["method"] == "Page.loadEventFired"
                        || (e["method"] == "Page.frameStoppedLoading"
                            && e["params"]["frameId"] == self.tab.target.as_str()))
            });
            match event {
                None => continue,
                Some(e) if e["method"] == "Page.loadEventFired" => return Ok(()),
                Some(_) => {
                    // Stopped without loading a page: a download, maybe,
                    // which the browser tells a moment later.
                    let soon = Instant::now() + crate::settings::get().download();
                    let target = self.tab.target.clone();
                    if let Some(download) = self.cdp.event(soon, |e| {
                        e["method"] == "Browser.downloadWillBegin"
                            && e["params"]["frameId"] == target.as_str()
                    }) {
                        notes.push(refused_download(&download)?);
                    }
                    return Ok(());
                }
            }
        }
        if !self.cdp.ended() {
            notes.push(format!(
                "The page was still loading after {}s.",
                crate::settings::get().load().as_secs()
            ));
        }
        Ok(())
    }

    /// Tells each dialog the page opened, as the connection answered it
    /// ([`answering`]).
    fn dialogs(&self, notes: &mut Vec<String>) -> Result<()> {
        let session = self.tab.session.clone();
        let dialogs = self.cdp.take(|e| {
            e["method"] == "Page.javascriptDialogOpening" && e["sessionId"] == session.as_str()
        });
        for dialog in dialogs {
            let params = &dialog["params"];
            let kind = match params["type"].as_str() {
                Some("beforeunload") => "leave-page question",
                Some(kind @ ("alert" | "confirm" | "prompt")) => kind,
                _ => "dialog",
            };
            let accepted = dialog["answered"]["accept"] == true;
            let how = match (kind, accepted) {
                ("alert", _) => "answered OK".to_string(),
                (_, true) => "answered OK, as 'dialog' said".to_string(),
                (_, false) => {
                    "dismissed. To answer it OK, call 'dialog' with answer ok, then do again what \
                     asked it"
                        .to_string()
                }
            };
            let message = params["message"].as_str().unwrap_or("");
            notes.push(format!(
                "The page's {kind} said this, {how}:\n{}",
                by_page(link_untrusted::Kind::Dialog, PAGE, message)?
            ));
        }
        Ok(())
    }

    /// What came of the last call: dialogs answered, downloads and file
    /// choosers refused.
    pub fn notes(&mut self, notes: &mut Vec<String>) -> Result<()> {
        self.dialogs(notes)?;
        let target = self.tab.target.clone();
        for download in self.cdp.take(|e| {
            e["method"] == "Browser.downloadWillBegin" && e["params"]["frameId"] == target.as_str()
        }) {
            notes.push(refused_download(&download)?);
        }
        let session = self.tab.session.clone();
        if !self
            .cdp
            .take(|e| e["method"] == "Page.fileChooserOpened" && e["sessionId"] == session.as_str())
            .is_empty()
        {
            notes.push("The page asked for a file, refused: this browser uploads none.".into());
        }
        Ok(())
    }

    /// The tool's world in the page, made when the page has none: a new
    /// page (a navigation) ends the last.
    fn world(&mut self) -> Result<i64> {
        if let Some(world) = self.tab.world {
            return Ok(world);
        }
        let made = self.cdp.call(
            "Page.createIsolatedWorld",
            json!({"frameId": self.tab.target, "worldName": WORLD}),
            Some(&self.tab.session),
            crate::settings::get().call(),
        )?;
        let world = made["executionContextId"]
            .as_i64()
            .context("the browser made no world")?;
        self.tab.world = Some(world);
        Ok(world)
    }

    /// The value of JavaScript `expression`, in the tool's world.
    fn evaluate(&mut self, expression: &str) -> Result<Value> {
        let world = self.world()?;
        let mut evaluated = self.evaluate_in(world, expression);
        // The page changed since: a new world, in the new page.
        if evaluated
            .as_ref()
            .is_err_and(|err| format!("{err:#}").contains("Cannot find context"))
        {
            self.tab.world = None;
            let world = self.world()?;
            evaluated = self.evaluate_in(world, expression);
        }
        let evaluated = evaluated?;
        if let Some(thrown) = evaluated.get("exceptionDetails") {
            bail!(
                "the page: {}",
                thrown["exception"]["description"]
                    .as_str()
                    .or(thrown["text"].as_str())
                    .unwrap_or("an error")
            );
        }
        Ok(evaluated["result"]["value"].clone())
    }

    fn evaluate_in(&self, world: i64, expression: &str) -> Result<Value> {
        self.cdp.call(
            "Runtime.evaluate",
            json!({"expression": expression, "contextId": world, "returnByValue": true,
                "awaitPromise": true}),
            Some(&self.tab.session),
            crate::settings::get().call(),
        )
    }

    pub fn read(&mut self) -> Result<Reading> {
        let read = self.evaluate(include_str!("page.js"))?;
        let note = crate::settings::get().note_bytes;
        Ok(Reading {
            title: crate::capped(read["title"].as_str().unwrap_or(""), note)
                .0
                .to_string(),
            url: crate::capped(read["url"].as_str().unwrap_or(""), note)
                .0
                .to_string(),
            text: read["text"].as_str().unwrap_or("").to_string(),
        })
    }

    /// Drops the page's loading said before now, so what an action
    /// starts is told apart from what came before it.
    fn forget_loading(&self) {
        let session = self.tab.session.clone();
        self.cdp.take(|e| {
            e["sessionId"] == session.as_str()
                && matches!(
                    e["method"].as_str(),
                    Some(
                        "Page.frameStartedLoading"
                            | "Page.frameStoppedLoading"
                            | "Page.loadEventFired"
                    )
                )
        });
    }

    /// Where part `n` is in the window, scrolled into view: refused
    /// when something else is on top of it there, which a click would
    /// land on instead.
    fn place(&mut self, n: u64) -> Result<(f64, f64)> {
        self.forget_loading();
        let at = self.evaluate(&format!("({})({n})", include_str!("place.js").trim()))?;
        let (Some(x), Some(y)) = (at["x"].as_f64(), at["y"].as_f64()) else {
            bail!("{}", gone(n));
        };
        if let Some(covered) = at["covered"].as_str() {
            bail!(
                "[{n}] is under another part of the page, which a click would land on \
                 instead; it's this:\n{}\nClose or scroll past what covers it, then try again",
                by_page(link_untrusted::Kind::Page, PAGE, covered)?
            );
        }
        Ok((x, y))
    }

    fn click_at(&self, x: f64, y: f64) -> Result<()> {
        for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
            self.cdp.call(
                "Input.dispatchMouseEvent",
                json!({"type": kind, "x": x, "y": y, "button": "left", "clickCount": 1}),
                Some(&self.tab.session),
                crate::settings::get().call(),
            )?;
        }
        Ok(())
    }

    /// Presses `name`, a key `browser.toml` names.
    fn key(&self, combo: &str) -> Result<()> {
        let press = pressed(combo)?;
        let call = |event: Value| {
            self.cdp.call(
                "Input.dispatchKeyEvent",
                event,
                Some(&self.tab.session),
                crate::settings::get().call(),
            )
        };
        // Each modifier held down in turn, the key pressed with them all,
        // then each let go, the last first.
        let mut held = 0;
        for modifier in &press.modifiers {
            held |= modifier.bit;
            call(
                json!({"type": "rawKeyDown", "key": modifier.name, "code": modifier.code,
                "windowsVirtualKeyCode": modifier.virtual_key, "modifiers": held}),
            )?;
        }
        for kind in ["keyDown", "keyUp"] {
            let mut event = json!({"type": kind, "key": press.key, "code": press.code,
                "windowsVirtualKeyCode": press.virtual_key, "modifiers": held});
            if kind == "keyDown" && !press.text.is_empty() {
                event["text"] = json!(press.text);
            }
            call(event)?;
        }
        for modifier in press.modifiers.iter().rev() {
            held &= !modifier.bit;
            call(
                json!({"type": "keyUp", "key": modifier.name, "code": modifier.code,
                "windowsVirtualKeyCode": modifier.virtual_key, "modifiers": held}),
            )?;
        }
        Ok(())
    }

    /// Why `url` wasn't reached, in our words: Chromium's `error`, and
    /// the browser's way out paused when it is.
    fn unreached(&self, url: &str, error: &str) -> String {
        if let Some(why) = crate::paused(self.sandbox) {
            return format!(
                "{url} wasn't opened: {}",
                crate::paused_note(self.sandbox, &why)
            );
        }
        format!("{url} wasn't opened: {}", unreached(error))
    }
}

/// A key `key` presses, with the modifiers held for it, as DevTools'
/// `Input.dispatchKeyEvent` takes them.
#[derive(Debug, PartialEq)]
pub(crate) struct Press {
    pub modifiers: Vec<Held>,
    /// The key as DevTools names it: the character it types, or its name.
    pub key: String,
    pub code: String,
    pub virtual_key: u32,
    /// What it types: none while Control, Alt or Meta is held.
    pub text: String,
}

/// A modifier held for a [`Press`].
#[derive(Debug, PartialEq)]
pub(crate) struct Held {
    pub name: String,
    pub code: String,
    pub virtual_key: u32,
    pub bit: u32,
}

/// `combo` as Playwright's `press` reads it: a key (`Enter`) or one
/// character (`a`), after any modifiers each joined by `+` (`Control+a`,
/// `Shift++`). A name it doesn't know is refused, saying what it takes.
pub(crate) fn pressed(combo: &str) -> Result<Press> {
    let settings = crate::settings::get();
    // `+` itself is a key: the last of `Shift++`, or the whole of `+`.
    let (modifiers, last) = match combo {
        "+" => ("", "+"),
        _ if combo.ends_with("++") => (&combo[..combo.len() - 2], "+"),
        _ => combo.rsplit_once('+').unwrap_or(("", combo)),
    };
    let mut held: Vec<Held> = Vec::new();
    for name in modifiers.split('+').filter(|_| !modifiers.is_empty()) {
        let modifier = settings.modifiers.get(name).with_context(|| {
            format!(
                "{name:?} in {combo:?} isn't a modifier: {}, each before + and the key",
                settings
                    .modifiers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        if held.iter().all(|h| h.name != name) {
            held.push(Held {
                name: name.to_string(),
                code: modifier.code.clone(),
                virtual_key: modifier.virtual_key,
                bit: modifier.bit,
            });
        }
    }
    let shifted = held.iter().any(|h| h.name == "Shift");
    let typing = held.iter().all(|h| h.name == "Shift");
    let mut chars = last.chars();
    let press = match (chars.next(), chars.next(), settings.keys.get(last)) {
        (_, _, Some(key)) => Press {
            modifiers: held,
            // DevTools names the space bar's key by the character it types.
            key: if key.text == " " {
                " ".into()
            } else {
                last.into()
            },
            code: key.code.clone(),
            virtual_key: key.virtual_key,
            text: if typing {
                key.text.clone()
            } else {
                String::new()
            },
        },
        (Some(c), None, None) if !c.is_control() => {
            let c = match shifted {
                true => c.to_ascii_uppercase(),
                false => c,
            };
            let upper = c.to_ascii_uppercase();
            let (code, virtual_key) = match upper {
                'A'..='Z' => (format!("Key{upper}"), u32::from(upper)),
                '0'..='9' => (format!("Digit{upper}"), u32::from(upper)),
                _ => (String::new(), 0),
            };
            Press {
                modifiers: held,
                key: c.to_string(),
                code,
                virtual_key,
                text: if typing { c.to_string() } else { String::new() },
            }
        }
        _ => bail!(
            "{last:?} isn't a key: one character, or {}",
            settings.keys.keys().cloned().collect::<Vec<_>>().join(", ")
        ),
    };
    Ok(press)
}

/// Part `n` isn't on the page: the numbers are a read's.
fn gone(n: u64) -> String {
    format!("there's no [{n}] on the page now: 'read' it again for its numbers")
}

/// Why a page wasn't reached, in our words: Chromium's `error`.
pub(crate) fn unreached(error: &str) -> &str {
    match error {
        "net::ERR_TUNNEL_CONNECTION_FAILED" | "net::ERR_PROXY_CONNECTION_FAILED" => {
            "refused: the browser reaches only the internet hosts its agent reaches, and \
             nothing on this machine or its network that wasn't named (--private-host), or \
             the host isn't there"
        }
        "net::ERR_NAME_NOT_RESOLVED" => "no such host",
        "net::ERR_CONNECTION_REFUSED" => "nothing answers there",
        "net::ERR_ABORTED" => "it's a download, or the page stopped it, refused",
        _ => error,
    }
}
