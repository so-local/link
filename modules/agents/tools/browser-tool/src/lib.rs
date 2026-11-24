//! Link browser: pages opened in a real browser, read, clicked, typed
//! into and seen as screenshots, for an agent to call as a tool
//! ([`mcp`]).
//!
//! The browser is Chromium, headless, started at the first call in a
//! sandbox of its own (`lnk sandbox run`): it reads its own program and
//! writes only its profile, and reaches the internet hosts the agent
//! reaches (`--internet-host`) through the sandbox's proxy, which keeps
//! the rule Link web keeps: nothing on this machine or its networks,
//! unless you name a host (`--private-host`). It's driven over
//! Chromium's DevTools protocol ([`cdp`]) on the sandbox's own input and
//! output, so no port is opened for it.
//!
//! Each conversation has a page of its own ([`Who`], [`tab`]): one the
//! agent's owner is answered in opens in the profile's own browser
//! context, so its sign-ins are kept from one session to the next; any
//! other in a context of its own, which keeps nothing and sees none of
//! them. A call that names no conversation is refused. At most
//! `pages_max` pages stay open (`browser.toml`), the one used least
//! recently closed past it, and one unused for `idle_ms` is closed.

pub mod cdp;
pub mod cli;
mod fencing;
mod guard;
pub mod mcp;
mod settings;
mod startup;
mod tab;
pub(crate) use fencing::by_page;
pub use guard::Private;

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use link_connection_contract::access::Standing;
use serde_json::{Value, json};

use crate::cdp::Cdp;
use crate::tab::{Page, Tab};
mod constants;

/// What the tool is told when it's defined.
#[derive(Clone, Debug)]
pub struct Config {
    /// The sandbox the browser runs in, `<lnk> sandbox run`, the
    /// browser's own flags put after it.
    pub sandbox: Vec<String>,
    /// The browser's program; found on PATH when not given.
    pub chromium: Option<PathBuf>,
    /// The browser's profile: its cookies and sign-ins, kept.
    pub profile: PathBuf,
    /// The internet's hosts it may reach, as the agent's `[internet]
    /// hosts` name them: `*` any. None: no internet.
    pub internet_hosts: Vec<String>,
    /// Hosts on this machine or its networks it may open: a host of
    /// your network (`nas.local`, `192.168.1.20`), or a port of this
    /// machine's (`localhost:8080`).
    pub private_hosts: Vec<String>,
    /// The agent's floor: the standing of a call that says none.
    pub floor: Standing,
}

/// Whose a call is: its conversation, and whether the message it
/// answers is its owner's. Each has a page of its own, so a member's
/// turn in a chat shared with the owner never reaches the owner's.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Who {
    /// The conversation, as its harness names it (`link.local/thread`),
    /// never empty.
    pub conversation: String,
    /// Whether it stands as the owner, and so uses the profile's
    /// sign-ins.
    pub owner: bool,
}

impl Who {
    /// Whose a call made with `meta` (its `_meta`) is: its conversation,
    /// and its standing, its harness's claim (`link.local/standing`) at
    /// most `floor`, or `floor` when it says none. A claim that isn't a
    /// standing stands as a stranger. A call that names no conversation
    /// is refused: it would share one page, and its sign-ins, with every
    /// other call naming none.
    pub fn of(meta: Option<&Value>, floor: Standing) -> Result<Who> {
        let meta = meta.unwrap_or(&Value::Null);
        let conversation = meta[link_connection_contract::access::THREAD_META]
            .as_str()
            .filter(|thread| !thread.is_empty())
            .context(
                "this call names no conversation (its _meta has no link.local/thread), and \
                 the browser gives each conversation a page of its own, so it opens none: \
                 use it through a harness that names the conversation, as Link Harness does",
            )?;
        let standing = match meta.get(link_connection_contract::access::STANDING_META) {
            None | Some(Value::Null) => floor,
            Some(claim) => serde_json::from_value::<Standing>(claim.clone())
                .map_or(Standing::Stranger, |claim| claim.at_most(floor)),
        };
        Ok(Who {
            conversation: conversation.to_string(),
            owner: standing == Standing::Owner,
        })
    }
}

/// The tool: what it was told, the browser while it runs, and each
/// conversation's page.
pub struct Browser {
    config: Config,
    private: Vec<Private>,
    engine: Option<Engine>,
    tabs: HashMap<Who, Tab>,
    /// Each page's answer to its next confirm, prompt, or leave-page
    /// question (`dialog`), by its session: dismissed when it has none.
    answers: tab::Answers,
    /// The sandbox of the browser that last ran: while its way out is
    /// paused, no other starts in its place.
    sandbox: Option<String>,
}

/// A browser running: its process, the connection to it, and its
/// sandbox's name (`run-<pid>`), as its proxy's pauses name it.
struct Engine {
    child: Option<Child>,
    cdp: Cdp,
    sandbox: String,
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            stop(&mut child);
        }
    }
}

/// Stops `child` and everything it started (its process group).
fn stop(child: &mut Child) {
    let group = child.id() as libc::pid_t;
    // Never group 1 or below: `kill(-1)` is every process.
    if group > 1 {
        // SAFETY: a signal to the process group the child leads.
        unsafe { libc::kill(-group, libc::SIGKILL) };
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// What a page read says: its title, its address and its text, the
/// clickable parts numbered.
#[derive(Clone, Debug)]
pub struct Reading {
    pub title: String,
    pub url: String,
    pub text: String,
}

impl Browser {
    pub fn new(config: Config) -> Result<Browser> {
        ensure!(
            !config.sandbox.is_empty(),
            "the browser runs only in a sandbox: none given"
        );
        guard::own_profile(&config.profile)?;
        let private = config
            .private_hosts
            .iter()
            .map(|h| Private::parse(h))
            .collect::<Result<_>>()?;
        Ok(Browser {
            config,
            private,
            engine: None,
            tabs: HashMap::new(),
            answers: Arc::new(Mutex::new(HashMap::new())),
            sandbox: None,
        })
    }

    /// Whose a call made with `meta` (its `_meta`) is, at most the
    /// agent's floor.
    pub fn who(&self, meta: Option<&Value>) -> Result<Who> {
        Who::of(meta, self.config.floor)
    }

    /// `who`'s page, the browser started if it isn't, or if it ended,
    /// and the page made if `who` has none.
    fn page(&mut self, who: &Who) -> Result<Page<'_>> {
        if self.engine.as_ref().is_some_and(|e| e.cdp.ended()) {
            self.engine = None;
            self.tabs.clear();
        }
        let now = Instant::now();
        let used: Vec<(Who, Instant)> = self
            .tabs
            .iter()
            .map(|(who, tab)| (who.clone(), tab.used))
            .collect();
        for gone in evicted(&used, who, now) {
            self.drop_tab(&gone);
        }
        if self.engine.is_none() {
            self.engine = Some(self.start()?);
        }
        let engine = self.engine.as_ref().expect("just started");
        self.sweep(engine);
        if !self.tabs.contains_key(who) {
            let tab = Tab::new(&engine.cdp, who.owner)?;
            self.tabs.insert(who.clone(), tab);
        }
        let tab = self.tabs.get_mut(who).expect("just made");
        tab.used = now;
        Ok(Page {
            cdp: &engine.cdp,
            tab,
            sandbox: &engine.sandbox,
        })
    }

    /// Drops the events of pages no conversation has now.
    fn sweep(&self, engine: &Engine) {
        let live = |key: &str, value: &Value| {
            self.tabs.values().any(|tab| match key {
                "sessionId" => value == tab.session.as_str(),
                _ => value == tab.target.as_str(),
            })
        };
        engine.cdp.take(|e| match e.get("sessionId") {
            Some(session) => !live("sessionId", session),
            None if e["method"] == "Target.targetCreated" => {
                !live("target", &e["params"]["targetInfo"]["openerId"])
            }
            None => !live("target", &e["params"]["frameId"]),
        });
    }

    fn start(&mut self) -> Result<Engine> {
        if let Some(sandbox) = &self.sandbox
            && let Some(why) = paused(sandbox)
        {
            bail!("{}", paused_note(sandbox, &why));
        }
        let chromium = chosen(self.config.chromium.clone())?;
        let real = std::fs::canonicalize(&chromium)
            .with_context(|| format!("the browser {}", chromium.display()))?;
        let profile = &self.config.profile;
        std::fs::create_dir_all(profile)
            .with_context(|| format!("creating {}", profile.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(profile, std::fs::Permissions::from_mode(0o700));
        }
        let argv = self.argv(&real);
        let mut command = link_secretive::process::command(&argv[0]);
        command
            .args(&argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let started = Instant::now();
        let mut child = command
            .spawn()
            .with_context(|| format!("starting {}", argv[0]))?;
        // `lnk sandbox run` names its sandbox by its process, which the
        // child is (each program before it hands over with exec).
        let sandbox = format!("run-{}", child.id());
        self.sandbox = Some(sandbox.clone());
        let stdin = child.stdin.take().context("the browser's input")?;
        let stdout = child.stdout.take().context("the browser's output")?;
        // Its last words, said should it not start.
        let said = Arc::new(Mutex::new(String::new()));
        let (done, drained) = std::sync::mpsc::channel();
        if let Some(mut stderr) = child.stderr.take() {
            let said = said.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n @ 1..) = stderr.read(&mut buf) {
                    let mut said = said.lock().unwrap_or_else(|e| e.into_inner());
                    said.push_str(&String::from_utf8_lossy(&buf[..n]));
                    let len = said.len();
                    if len > 8192 {
                        let cut = said.ceil_char_boundary(len - 4096);
                        said.drain(..cut);
                    }
                }
                let _ = done.send(());
            });
        } else {
            drop(done);
        }
        let keep = cdp::Keep {
            methods: settings::get().events.clone(),
            bytes: settings::get().events_bytes,
            message: settings::get().message_bytes,
            answer: tab::answering(self.answers.clone()),
        };
        let mut engine = Engine {
            child: Some(child),
            cdp: Cdp::new(stdin, stdout, keep),
            sandbox,
        };
        let version = match engine.cdp.call(
            "Browser.getVersion",
            json!({}),
            None,
            crate::settings::get().start(),
        ) {
            Ok(version) => version,
            Err(err) => {
                if let Some(mut child) = engine.child.take() {
                    stop(&mut child);
                }
                let _ = drained.recv_timeout(
                    crate::settings::get()
                        .start()
                        .saturating_sub(started.elapsed()),
                );
                let said = said.lock().unwrap_or_else(|e| e.into_inner()).clone();
                bail!("{}", startup::failure(&err, &said));
            }
        };
        supported(version["product"].as_str().unwrap_or_default(), &real)?;
        // No downloads in the profile's context: refused, and said
        // ([`Page::notes`]). Each other context refuses its own.
        tab::deny_downloads(&engine.cdp, None)?;
        engine.cdp.call(
            "Target.setDiscoverTargets",
            json!({"discover": true}),
            None,
            crate::settings::get().call(),
        )?;
        Ok(engine)
    }

    /// The command starting the browser `real` (its program, links
    /// resolved): in its sandbox, its DevTools on the sandbox's input and
    /// output (descriptors 3 and 4 inside), its proxy the sandbox's.
    fn argv(&self, real: &Path) -> Vec<String> {
        let mut argv = self.config.sandbox.clone();
        // Its program's folder alone, never the folder a link to it is
        // in, where it isn't among the system's programs the sandbox
        // reads anyway.
        let folder = program_folder(real);
        if let Some(folder) = folder.filter(|f| !system(f)) {
            argv.extend(["--files.read".into(), folder.display().to_string()]);
        }
        // What it runs, named: the shell that starts it, its program, and
        // its folder, where its helpers are.
        argv.extend(["--commands.allow".into(), "/bin/sh".into()]);
        argv.extend(["--commands.allow".into(), real.display().to_string()]);
        // Never a folder of the system's programs, as a Snap's engine is
        // in `/usr/bin`: that would allow every program there.
        if let Some(folder) = folder.filter(|f| {
            !constants::PROGRAM_FOLDERS
                .iter()
                .any(|p| *f == Path::new(p))
        }) {
            argv.extend(["--commands.allow".into(), format!("{}/", folder.display())]);
        }
        argv.extend([
            "--files.write".into(),
            self.config.profile.display().to_string(),
        ]);
        // The agent's internet hosts, and no more.
        for host in &self.config.internet_hosts {
            argv.extend(["--internet.hosts".into(), host.clone()]);
        }
        for private in &self.private {
            argv.extend(private.flags());
        }
        argv.extend([
            "--".into(),
            "/bin/sh".into(),
            "-c".into(),
            "exec \"$0\" --proxy-server=\"$HTTPS_PROXY\" \"$@\" 3<&0 4>&1 0</dev/null 1>/dev/null"
                .into(),
            real.display().to_string(),
            "--headless".into(),
            "--remote-debugging-pipe".into(),
            format!("--user-data-dir={}", self.config.profile.display()),
            format!(
                "--window-size={},{}",
                crate::settings::get().width,
                crate::settings::get().height
            ),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            "--disable-background-networking".into(),
            "--disable-dev-shm-usage".into(),
            "--disable-extensions".into(),
            // Its saved passwords and cookies' key in its profile, not
            // the system's keychain, which its sandbox doesn't reach.
            "--use-mock-keychain".into(),
            "--password-store=basic".into(),
            "--mute-audio".into(),
            "--hide-scrollbars".into(),
        ]);
        if !own_sandbox() {
            argv.push("--no-sandbox".into());
        }
        argv.push("about:blank".into());
        argv
    }

    /// Opens `url` in `who`'s page, waits for it to load, and reads it.
    pub fn go(&mut self, who: &Who, url: &str) -> Result<(Reading, Vec<String>)> {
        let url = guard::allowed(url, &self.private)?;
        self.page(who)?.go(&url)
    }

    /// `who`'s page's text, its clickable parts numbered.
    pub fn read(&mut self, who: &Who) -> Result<(Reading, Vec<String>)> {
        let mut page = self.page(who)?;
        let mut notes = Vec::new();
        page.notes(&mut notes)?;
        Ok((page.read()?, notes))
    }

    /// Clicks part `n` of the last read.
    pub fn click(&mut self, who: &Who, n: u64) -> Result<Vec<String>> {
        self.page(who)?.click(n)
    }

    /// Types `text` into part `n`, what it held replaced, and presses
    /// Enter after with `submit`.
    pub fn type_into(
        &mut self,
        who: &Who,
        n: u64,
        text: &str,
        submit: bool,
    ) -> Result<Vec<String>> {
        self.page(who)?.type_into(n, text, submit)
    }

    /// Chooses `values` (each an option's text or value) in choice `n`.
    pub fn select(&mut self, who: &Who, n: u64, values: &[String]) -> Result<Vec<String>> {
        self.page(who)?.select(n, values)
    }

    /// Scrolls part `n` into view, or the page `down` or up by most of a
    /// window. Where it is then.
    pub fn scroll(&mut self, who: &Who, n: Option<u64>, down: bool) -> Result<Vec<String>> {
        self.page(who)?.scroll(n, down)
    }

    /// Presses key `name`, or a combination (`Control+a`), on `who`'s
    /// page.
    pub fn key(&mut self, who: &Who, name: &str) -> Result<Vec<String>> {
        self.page(who)?.press(name)
    }

    /// Goes back a page on `who`'s page.
    pub fn back(&mut self, who: &Who) -> Result<Vec<String>> {
        self.page(who)?.back()
    }

    /// Moves the pointer over part `n` of `who`'s page.
    pub fn hover(&mut self, who: &Who, n: u64) -> Result<Vec<String>> {
        self.page(who)?.hover(n)
    }

    /// What `who`'s window shows, as a PNG.
    pub fn screenshot(&mut self, who: &Who) -> Result<(Vec<u8>, Vec<String>)> {
        self.page(who)?.screenshot()
    }

    /// How `who`'s page answers its next confirm, prompt, or leave-page
    /// question: OK (`accept`, with `text` for a prompt), or dismissed.
    pub fn dialog(&mut self, who: &Who, accept: bool, text: Option<&str>) -> Result<Vec<String>> {
        let session = self.page(who)?.tab.session.clone();
        let answer = tab::Answer {
            accept,
            text: text.map(str::to_string),
        };
        self.answers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session, answer);
        Ok(vec![match accept {
            true => "The page's next confirm, prompt, or leave-page question will be answered \
                     OK: do again what asked it."
                .into(),
            false => "The page's next confirm, prompt, or leave-page question will be \
                      dismissed, as each is unless 'dialog' says OK."
                .into(),
        }])
    }

    /// Closes `who`'s page; the profile stays. The browser closes with
    /// the last page. Whether `who` had one.
    pub fn close(&mut self, who: &Who) -> bool {
        let had = self.drop_tab(who);
        if had && self.tabs.is_empty() {
            self.quit();
        }
        had
    }

    /// Closes `who`'s page, and its context, the browser left running:
    /// whether it had one.
    fn drop_tab(&mut self, who: &Who) -> bool {
        let Some(tab) = self.tabs.remove(who) else {
            return false;
        };
        if let Some(engine) = &self.engine {
            tab.close(&engine.cdp);
            self.answers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&tab.session);
        }
        true
    }

    /// Closes the browser, every conversation's page with it; the
    /// profile stays.
    #[expect(
        clippy::disallowed_methods,
        reason = "polls a closing browser until it ends"
    )]
    pub fn quit(&mut self) {
        self.tabs.clear();
        let Some(mut engine) = self.engine.take() else {
            return;
        };
        let settings = settings::get();
        let _ = engine
            .cdp
            .call("Browser.close", json!({}), None, settings.close());
        let deadline = Instant::now() + settings.end();
        while Instant::now() < deadline {
            if engine
                .child
                .as_mut()
                .is_none_or(|child| child.try_wait().ok().flatten().is_some())
            {
                break;
            }
            std::thread::sleep(settings.end_poll());
        }
    }
}

/// The pages of `used` (each conversation's, and when it was last used)
/// to close before `who`'s call at `now`: each unused for `idle_ms`, and
/// past `pages_max`, with `who`'s counted, the least recently used. Never
/// `who`'s own.
fn evicted(used: &[(Who, Instant)], who: &Who, now: Instant) -> Vec<Who> {
    let settings = settings::get();
    let mut others: Vec<&(Who, Instant)> = used.iter().filter(|(w, _)| w != who).collect();
    others.sort_by_key(|(_, at)| *at);
    let room = settings.pages_max.saturating_sub(1);
    let over = others.len().saturating_sub(room);
    others
        .iter()
        .enumerate()
        .filter(|(n, (_, at))| *n < over || now.saturating_duration_since(*at) >= settings.idle())
        .map(|(_, (w, _))| w.clone())
        .collect()
}

/// Why sandbox `sandbox`'s way out is paused, while it is.
fn paused(sandbox: &str) -> Option<String> {
    let dir = link_sandbox_client::wire_dir().ok()?;
    let wires = link_sandbox_client::Wires::read(&dir).ok()?;
    wires
        .paused
        .into_iter()
        .find(|p| p.sandbox == sandbox)
        .map(|p| p.why)
}

/// The browser's way out paused, said with how it opens again.
fn paused_note(sandbox: &str, why: &str) -> String {
    format!(
        "the browser's way out is paused ({why}), so no page opens: its sandbox's proxy \
         pauses it when a page reaches for what no normal page does. Tell your owner, who \
         can see why with lnk sandbox list --paused and open it again with lnk sandbox start {sandbox}"
    )
}

/// Refuses Chromium `program`, its DevTools' `product`
/// (`HeadlessChrome/131.0.6778.33`), below the oldest major version
/// supported.
fn supported(product: &str, program: &Path) -> Result<()> {
    let least = settings::get().minimum_major;
    let major = product
        .rsplit_once('/')
        .and_then(|(_, version)| version.split('.').next())
        .and_then(|major| major.parse::<u32>().ok());
    match major {
        Some(major) if major >= least => Ok(()),
        _ => bail!(
            "{} is {product:?}, and the browser starts Chromium {least} or newer: it runs \
             without Chromium's own sandbox, so an older one's known holes would be open. \
             Update it, or name a newer one ([tool.browser] program in the agent's spec)",
            program.display()
        ),
    }
}

/// The folder granted to `program`: a macOS executable loads frameworks
/// and helper programs elsewhere in its app bundle, so that bundle,
/// never the Applications folder; any other, its own folder.
fn program_folder(program: &Path) -> Option<&Path> {
    program
        .ancestors()
        .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
        .last()
        .or_else(|| program.parent())
}

/// Whether `folder` is among the system's programs the sandbox reads.
fn system(folder: &Path) -> bool {
    ["/usr", "/bin", "/sbin", "/lib", "/lib64"]
        .iter()
        .any(|root| folder.starts_with(root))
}

/// The browser's program: the one given (`--chromium`, an agent's
/// `[tool.browser] program`), else the one found here, a distribution
/// launcher resolved to its engine either way.
fn chosen(given: Option<PathBuf>) -> Result<PathBuf> {
    let program = match given {
        Some(program) => program,
        None => find_chromium().context(
            "no Chromium here: install Chromium or Google Chrome, or name its program in the \
             agent's spec ([tool.browser] program) or with lnk-browser-tool serve --chromium",
        )?,
    };
    Ok(chromium_program(program))
}

/// Resolve a distribution launcher to its engine, preserving other programs.
fn chromium_program(program: PathBuf) -> PathBuf {
    let real = std::fs::canonicalize(&program).unwrap_or_else(|_| program.clone());
    for (launcher, engine) in constants::PACKAGED_CHROMIUMS {
        if std::fs::canonicalize(launcher).is_ok_and(|path| path == real)
            && Path::new(engine).is_file()
        {
            return PathBuf::from(engine);
        }
    }
    program
}

/// Whether Chromium may start its own sandbox inside Link's. Never on
/// Linux: Chromium's sandbox needs a user namespace, which Link's sandbox
/// refuses inside (bubblewrap's `--disable-userns` and its seccomp filter),
/// so Chromium would stop with "No usable sandbox!". Nor as root, where
/// Chromium refuses its own. Link's sandbox is the wall either way.
fn own_sandbox() -> bool {
    if cfg!(target_os = "linux") {
        return false;
    }
    #[cfg(unix)]
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return false;
    }
    true
}

/// Chromium on this machine: on PATH, or where a Mac keeps it.
pub fn find_chromium() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for name in constants::CHROMIUMS {
        for dir in std::env::split_paths(&path) {
            let program = dir.join(name);
            if program.is_file() {
                return Some(chromium_program(program));
            }
        }
    }
    constants::MAC_CHROMIUMS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

/// Of `text`, the part from byte `start`, at most `most` bytes, cut at a
/// line's end where one is near: the part, and where the next starts
/// (none at the end).
pub fn part(text: &str, start: usize, most: usize) -> (&str, Option<usize>) {
    let start = text.floor_char_boundary(start.min(text.len()));
    let rest = &text[start..];
    if rest.len() <= most {
        return (rest, None);
    }
    let end = rest.floor_char_boundary(most);
    let end = match rest[..end].rfind('\n') {
        Some(line) if line > most / 2 => line + 1,
        _ => end,
    };
    (&rest[..end], Some(start + end))
}

/// Of `text`, at most `most` bytes, cut at a character, and how many
/// bytes were cut.
pub(crate) fn capped(text: &str, most: usize) -> (&str, usize) {
    let end = text.floor_char_boundary(most.min(text.len()));
    (&text[..end], text.len() - end)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(unit);
