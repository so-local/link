//! Link desktop: a desktop an agent sees and works, as screenshots,
//! mouse and keys, for it to call as a tool ([`mcp`]).
//!
//! The desktop is a virtual one: an `Xvfb` display this tool starts at
//! the first call, in a sandbox of its own (`lnk sandbox run`), where the
//! programs the agent runs with Link run show, so it works on a machine
//! with no screen and touches nothing of yours. The display takes only
//! clients holding its cookie, kept in a file only your user reads and
//! renewed at each start ([`auth`]), so no other user of the machine sees
//! or draws on it; and the cookie goes only to an X server run by your
//! user. No window manager runs: windows have no frames, and
//! nothing closes one at a key. Input goes through the XTEST extension,
//! as a person's would; a screenshot is read from the display's root
//! window.

pub mod auth;
pub mod cli;
mod connection;
pub mod keys;
pub mod limits;
pub mod mcp;
pub mod png;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{self, ConnectionExt as _, ImageFormat, Keycode};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use crate::connection::{Timed, connect};

use crate::limits::limits;

/// The X server's event types XTEST takes.
const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;
const BUTTON_PRESS: u8 = 4;
const BUTTON_RELEASE: u8 = 5;
const MOTION: u8 = 6;

/// Where display `n`'s socket is, in the folder `limits.toml` names.
pub fn socket(display: u32) -> PathBuf {
    Path::new(&limits().sockets).join(format!("X{display}"))
}

/// What the tool is told when it's defined.
#[derive(Clone, Debug)]
pub struct Config {
    /// The display's number, `:<n>`.
    pub display: u32,
    /// The display's cookie's file ([`auth`]).
    pub auth: PathBuf,
    /// `Xvfb`'s program; found in the system's folders when not given
    /// (`limits.toml`), never on PATH.
    pub xvfb: Option<PathBuf>,
    /// The sandbox `Xvfb` runs in, `<lnk> sandbox run` and its flags,
    /// the program after them.
    pub sandbox: Vec<String>,
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Button {
    Left = 1,
    Middle = 2,
    Right = 3,
}

/// A way to scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scroll {
    Up = 4,
    Down = 5,
    Left = 6,
    Right = 7,
}

/// The tool: what it was told, and the display while it's open.
pub struct Desktop {
    config: Config,
    open: Option<Open>,
    /// Whether the display was started again since a call last asked:
    /// the programs that were on it are gone.
    restarted: bool,
}

/// The display, and the connection to it.
struct Open {
    /// The `Xvfb` this tool started, if it did; one already there is
    /// left as it is. Held for its drop, which stops it.
    _server: Option<Server>,
    conn: RustConnection<Timed>,
    root: xproto::Window,
    width: u16,
    height: u16,
}

/// The `Xvfb` this tool started, in its sandbox, and the display's lock
/// and socket it leaves while it runs, gone with it.
struct Server {
    child: Child,
    lock: PathBuf,
    socket: PathBuf,
    /// The end of what it wrote on its errors, once they close: read all
    /// along, so a server that writes much never blocks on a full pipe.
    errors: mpsc::Receiver<Vec<u8>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        stop(&mut self.child);
        // A server its sandbox ended leaves its socket; the lock is this
        // tool's.
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_file(&self.lock);
    }
}

impl Desktop {
    pub fn new(config: Config) -> Desktop {
        Desktop {
            config,
            open: None,
            restarted: false,
        }
    }

    /// The display, started if it isn't.
    fn open(&mut self) -> Result<&Open> {
        let had = self.open.is_some();
        let alive = self
            .open
            .as_ref()
            .is_some_and(|o| o.conn.get_input_focus().is_ok_and(|c| c.reply().is_ok()));
        if !alive {
            self.open = None;
            let open = start(&self.config)?;
            // A display that was open, started again by this tool: what
            // ran on it ended with the server before.
            self.restarted |= had && open._server.is_some();
            self.open = Some(open);
        }
        Ok(self.open.as_ref().expect("just opened"))
    }

    /// Whether the display was started again since this was last asked,
    /// the programs that were on it gone; asked once, it's forgotten.
    pub fn restarted(&mut self) -> bool {
        std::mem::take(&mut self.restarted)
    }

    /// What the display shows, as a PNG, and its size.
    pub fn screenshot(&mut self) -> Result<(Vec<u8>, u16, u16)> {
        let open = self.open()?;
        let image = open
            .conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                open.root,
                0,
                0,
                open.width,
                open.height,
                !0,
            )?
            .reply()
            .context("reading the display")?;
        let (width, height) = (open.width as usize, open.height as usize);
        let bytes = image.data.len() / (width * height).max(1);
        ensure!(
            bytes == 4,
            "the display's pixels are {bytes} bytes: only 32-bit pixels are read"
        );
        // Each pixel BGRX, least significant byte first, as Xvfb keeps
        // them on this machine.
        let mut rgb = Vec::with_capacity(width * height * 3);
        for pixel in image.data.as_chunks::<4>().0 {
            rgb.extend([pixel[2], pixel[1], pixel[0]]);
        }
        Ok((
            png::encode(open.width as u32, open.height as u32, &rgb),
            open.width,
            open.height,
        ))
    }

    /// Moves the mouse to (`x`, `y`) and clicks `button`, twice with
    /// `double`.
    pub fn click(&mut self, x: u16, y: u16, button: Button, double: bool) -> Result<()> {
        let open = self.open()?;
        open.inside(x, y)?;
        open.motion(x, y)?;
        for _ in 0..if double { 2 } else { 1 } {
            open.fake(BUTTON_PRESS, button as u8)?;
            open.fake(BUTTON_RELEASE, button as u8)?;
        }
        open.sync()
    }

    /// Presses the left button at one place and releases it at another,
    /// released though a step on the way fails.
    #[expect(clippy::disallowed_methods, reason = "paces a drag, as a hand moves")]
    pub fn drag(&mut self, from: (u16, u16), to: (u16, u16)) -> Result<()> {
        let open = self.open()?;
        open.inside(from.0, from.1)?;
        open.inside(to.0, to.1)?;
        open.motion(from.0, from.1)?;
        held(
            || open.fake(BUTTON_PRESS, Button::Left as u8),
            // In steps, as a hand moves: a program that follows the drag
            // sees it go.
            || {
                open.sync()?;
                let steps = i32::from(limits().drag_steps.max(1));
                for step in 1..=steps {
                    let at =
                        |a: u16, b: u16| (a as i32 + (b as i32 - a as i32) * step / steps) as u16;
                    open.motion(at(from.0, to.0), at(from.1, to.1))?;
                    open.sync()?;
                    std::thread::sleep(Duration::from_millis(limits().drag_step_ms));
                }
                Ok(())
            },
            || open.fake(BUTTON_RELEASE, Button::Left as u8),
        )?;
        open.sync()
    }

    /// Scrolls `clicks` of the wheel `way`, at (`x`, `y`) when given;
    /// `actions.toml` bounds how many.
    pub fn scroll(&mut self, at: Option<(u16, u16)>, way: Scroll, clicks: u32) -> Result<()> {
        let open = self.open()?;
        if let Some((x, y)) = at {
            open.inside(x, y)?;
            open.motion(x, y)?;
        }
        for _ in 0..clicks {
            open.fake(BUTTON_PRESS, way as u8)?;
            open.fake(BUTTON_RELEASE, way as u8)?;
        }
        open.sync()
    }

    /// Types `text`, a key for each character, shifted where it must be.
    /// The keymap is read once for all of it.
    pub fn type_text(&mut self, text: &str) -> Result<()> {
        let open = self.open()?;
        let mut map = open.keymap()?;
        let mut shift = None;
        let typed = (|| -> Result<()> {
            for keysym in keys::characters(text) {
                let key = open.key(&mut map, keysym)?;
                let shifted = match key.shifted {
                    true => {
                        if shift.is_none() {
                            shift = Some(open.key(&mut map, keys::named("shift")?)?);
                        }
                        shift
                    }
                    false => None,
                };
                let pressed = open.press(shifted.iter().map(|k| k.code).chain([key.code]));
                if key.borrowed {
                    open.give_back(&mut map, key.code)?;
                }
                pressed?;
            }
            Ok(())
        })();
        if let Some(shift) = shift.filter(|k| k.borrowed) {
            open.give_back(&mut map, shift.code)?;
        }
        typed
    }

    /// Presses the keys of `combination` (`ctrl+c`) in order, and
    /// releases them in turn. Every key is looked up before any is
    /// pressed, so one the desktop can't give leaves none held; and what
    /// was pressed is released whatever fails after.
    pub fn key(&mut self, combination: &str) -> Result<()> {
        let keysyms = keys::combination(combination)?;
        let open = self.open()?;
        let mut map = open.keymap()?;
        let mut codes = Vec::new();
        let mut borrowed = Vec::new();
        let looked = (|| -> Result<()> {
            for keysym in keysyms {
                let key = open.key(&mut map, keysym)?;
                if key.borrowed {
                    borrowed.push(key.code);
                }
                if key.shifted {
                    let shift = open.key(&mut map, keys::named("shift")?)?;
                    if shift.borrowed {
                        borrowed.push(shift.code);
                    }
                    codes.push(shift.code);
                }
                codes.push(key.code);
            }
            Ok(())
        })();
        let pressed = looked.and_then(|()| open.press(codes));
        // Every borrowed key is given back, though one fails.
        let given = borrowed
            .into_iter()
            .map(|code| open.give_back(&mut map, code))
            .fold(Ok(()), Result::and);
        pressed.and(given)
    }

    /// The display's size, started if it isn't.
    pub fn size(&mut self) -> Result<(u16, u16)> {
        let open = self.open()?;
        Ok((open.width, open.height))
    }
}

impl Open {
    fn inside(&self, x: u16, y: u16) -> Result<()> {
        ensure!(
            x < self.width && y < self.height,
            "({x}, {y}) is off the desktop, which is {} by {}",
            self.width,
            self.height
        );
        Ok(())
    }

    fn fake(&self, kind: u8, detail: u8) -> Result<()> {
        self.conn
            .xtest_fake_input(kind, detail, 0, self.root, 0, 0, 0)?;
        Ok(())
    }

    fn motion(&self, x: u16, y: u16) -> Result<()> {
        self.conn
            .xtest_fake_input(MOTION, 0, 0, self.root, x as i16, y as i16, 0)?;
        Ok(())
    }

    /// Waits until the display has done what was asked.
    fn sync(&self) -> Result<()> {
        self.conn.get_input_focus()?.reply()?;
        Ok(())
    }

    /// Presses `codes` in order, then releases each pressed, last first:
    /// a failure partway still releases what was pressed, so no modifier
    /// stays held for later input.
    fn press(&self, codes: impl IntoIterator<Item = Keycode>) -> Result<()> {
        let mut pressed = Vec::new();
        let mut done = Ok(());
        for code in codes {
            if let Err(err) = self.fake(KEY_PRESS, code) {
                done = Err(err);
                break;
            }
            pressed.push(code);
        }
        for code in pressed.into_iter().rev() {
            let released = self.fake(KEY_RELEASE, code);
            done = done.and(released);
        }
        done.and_then(|()| self.sync())
    }

    /// The display's keyboard mapping, read once for a call's keys.
    fn keymap(&self) -> Result<Keymap> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let mapping = self
            .conn
            .get_keyboard_mapping(min, max - min + 1)?
            .reply()?;
        Ok(Keymap {
            min,
            per: usize::from(mapping.keysyms_per_keycode).max(1),
            keysyms: mapping.keysyms,
        })
    }

    /// The key that gives `keysym` in `map`, and whether with shift. A
    /// keysym no key gives is put on a key no keysym is on, borrowed
    /// until it's typed, then given back ([`Open::give_back`]). A spare
    /// key is read again from the display before it's borrowed, since a
    /// program there may have mapped it since the call read the keymap;
    /// and one mapped whose change the display didn't confirm is given
    /// back before the call fails.
    #[expect(
        clippy::disallowed_methods,
        reason = "programs read a new key mapping a moment after they're told"
    )]
    fn key(&self, map: &mut Keymap, keysym: u32) -> Result<Key> {
        let (min, per) = (map.min, map.per);
        let mut spares = Vec::new();
        for (n, syms) in map.keysyms.chunks(per).enumerate() {
            let code = min + n as u8;
            if syms.first() == Some(&keysym) {
                return Ok(Key {
                    code,
                    shifted: false,
                    borrowed: false,
                });
            }
            if syms.get(1) == Some(&keysym) {
                return Ok(Key {
                    code,
                    shifted: true,
                    borrowed: false,
                });
            }
            if syms.iter().all(|s| *s == 0) {
                spares.push(code);
            }
        }
        let mut free = None;
        for code in spares {
            let now = self.conn.get_keyboard_mapping(code, 1)?.reply()?.keysyms;
            if now.iter().all(|s| *s == 0) {
                free = Some(code);
                break;
            }
            // Mapped since by a program on the display: kept as it is.
            map.copy(code, &now);
        }
        let code = free.context("no key is free to type that character on")?;
        let syms = vec![keysym; per];
        self.conn
            .change_keyboard_mapping(1, code, per as u8, &syms)?;
        map.set(code, keysym);
        if let Err(err) = self.sync() {
            let _ = self.give_back_now(map, code);
            return Err(err);
        }
        // Programs read the new mapping as they're told of it.
        std::thread::sleep(Duration::from_millis(limits().key_mapping_ms));
        Ok(Key {
            code,
            shifted: false,
            borrowed: true,
        })
    }

    /// Frees borrowed key `code` again, once what was typed on it is
    /// done: a desktop has a few spare keys, and every character the
    /// keymap lacks takes one.
    #[expect(
        clippy::disallowed_methods,
        reason = "programs read a typed key's mapping a moment after its events"
    )]
    fn give_back(&self, map: &mut Keymap, code: Keycode) -> Result<()> {
        // The program typed into reads the key as it was mapped first.
        std::thread::sleep(Duration::from_millis(limits().key_mapping_ms));
        self.give_back_now(map, code)
    }

    /// [`Open::give_back`], with no wait: for a key nothing was typed on.
    fn give_back_now(&self, map: &mut Keymap, code: Keycode) -> Result<()> {
        self.conn
            .change_keyboard_mapping(1, code, map.per as u8, &vec![0; map.per])?;
        map.set(code, 0);
        self.sync()
    }
}

/// The display's keyboard mapping as a call last read or changed it:
/// each key's keysyms, `per` a key, from key `min`.
struct Keymap {
    min: Keycode,
    per: usize,
    keysyms: Vec<u32>,
}

impl Keymap {
    /// Puts `keysym` (0, none) on every level of key `code`.
    fn set(&mut self, code: Keycode, keysym: u32) {
        if let Some(syms) = self.levels(code) {
            syms.fill(keysym);
        }
    }

    /// Puts what the display says is on key `code` in the call's copy.
    fn copy(&mut self, code: Keycode, now: &[u32]) {
        if let Some(syms) = self.levels(code) {
            for (sym, level) in syms.iter_mut().zip(now.iter().chain(std::iter::repeat(&0))) {
                *sym = *level;
            }
        }
    }

    fn levels(&mut self, code: Keycode) -> Option<&mut [u32]> {
        let at = usize::from(code.checked_sub(self.min)?) * self.per;
        self.keysyms.get_mut(at..at + self.per)
    }
}

/// A key that types a keysym: its code, whether with shift, and whether
/// it was borrowed, a spare key mapped for it, to be given back.
#[derive(Clone, Copy)]
struct Key {
    code: Keycode,
    shifted: bool,
    borrowed: bool,
}

/// Starts display `config.display`, or uses the one there that takes
/// its cookie, run by your user, and connects to it.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a starting Xvfb until it serves"
)]
fn start(config: &Config) -> Result<Open> {
    ensure!(
        !config.sandbox.is_empty(),
        "the desktop's X server runs only in a sandbox: none given"
    );
    let socket = socket(config.display);
    let refused = match connect(&socket, &auth::read(&config.auth)?, None) {
        Ok(conn) => return opened(conn, None),
        Err(err) => err,
    };
    let lock = limits().lock(config.display);
    stale(&lock, &socket);
    if lock.exists() || socket.exists() {
        bail!(
            "display :{} is taken by another X server here ({refused:#}): the agent's desktop \
             needs it",
            config.display
        );
    }
    let xvfb = match &config.xvfb {
        Some(program) => program.clone(),
        None => system_xvfb().with_context(|| {
            format!(
                "no Xvfb in the system's folders ({}): install it (apt install xvfb, dnf install \
                 xorg-x11-server-Xvfb) or name its program (lnk-desktop-tool serve --xvfb \
                 <program>)",
                limits().xvfb_folders.join(", ")
            )
        })?,
    };
    sockets(&socket)?;
    // The display's lock, this tool's: the X server in its sandbox
    // writes its own in a /tmp no other process sees.
    hold(&lock)?;
    // A new cookie for each server this tool starts, once the display is
    // its own: one copied while an earlier server ran opens no later one,
    // and a start that lost the display leaves the cookie as it was.
    let cookie = auth::renew(&config.auth, config.display)
        .and_then(|()| auth::read(&config.auth))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&lock);
        })?;
    let argv = xvfb_argv(config, &xvfb);
    let mut command = link_secretive::process::command(&argv[0]);
    command
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    // Its sandbox and what that starts, a group of their own, stopped
    // together.
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    // It ends with this tool, however the tool ends: the sandbox ends
    // what it runs with it.
    #[cfg(target_os = "linux")]
    // SAFETY: only prctl between fork and exec.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut command, || {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            let _ = std::fs::remove_file(&lock);
            return Err(err).with_context(|| format!("starting {}", argv[0]));
        }
    };
    let (kept, errors) = mpsc::channel();
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            let _ = kept.send(keep_errors(stderr, limits().errors_kept));
        });
    }
    let mut server = Server {
        child,
        lock,
        socket: socket.clone(),
        errors,
    };
    let started = server.child.id();
    let start = Duration::from_millis(limits().start_ms);
    let deadline = Instant::now() + start;
    loop {
        if let Ok(conn) = connect(&socket, &cookie, Some(started)) {
            return opened(conn, Some(server));
        }
        if let Some(status) = server.child.try_wait()? {
            let said = server
                .errors
                .recv_timeout(Duration::from_millis(limits().reply_ms))
                .unwrap_or_default();
            let said = String::from_utf8_lossy(&said);
            let said: Vec<&str> = said.lines().filter(|l| !l.trim().is_empty()).collect();
            bail!(
                "Xvfb ended ({status}): {}",
                said[said.len().saturating_sub(3)..].join(" / ")
            );
        }
        if Instant::now() > deadline {
            bail!("Xvfb didn't start within {}s", start.as_secs());
        }
        std::thread::sleep(Duration::from_millis(limits().start_poll_ms));
    }
}

/// The command starting `xvfb` on `config.display` under `config.sandbox`
/// (`lnk sandbox run`), as Link browser runs Chromium: it reads its
/// program and the display's cookie, and writes only X's folder of
/// sockets, where its own is made for programs to reach; no network, and
/// its lock and keymaps in a `/tmp` of its own.
fn xvfb_argv(config: &Config, xvfb: &Path) -> Vec<String> {
    let mut argv = config.sandbox.clone();
    let real = std::fs::canonicalize(xvfb).unwrap_or_else(|_| xvfb.to_path_buf());
    let mut folders: Vec<&Path> = [xvfb, real.as_path()]
        .into_iter()
        .filter_map(Path::parent)
        .filter(|folder| !system(folder))
        .collect();
    folders.dedup();
    for folder in folders {
        argv.extend(["--files.read".into(), folder.display().to_string()]);
    }
    argv.extend(["--files.read".into(), config.auth.display().to_string()]);
    // What it runs, named: its program, and the desktop toolchain's
    // (the keymap compiler and the shell it runs that with).
    let mut programs = vec![xvfb, real.as_path()];
    programs.dedup();
    for program in programs {
        argv.extend(["--commands.allow".into(), program.display().to_string()]);
    }
    argv.extend(["--commands.toolchains".into(), "desktop".into()]);
    if let Some(sockets) = socket(config.display).parent() {
        argv.extend(["--files.write".into(), sockets.display().to_string()]);
    }
    argv.extend([
        "--".into(),
        xvfb.display().to_string(),
        format!(":{}", config.display),
        "-screen".into(),
        "0".into(),
        {
            let (width, height) = limits::size();
            format!("{width}x{height}x24")
        },
        "-auth".into(),
        config.auth.display().to_string(),
        "-nolisten".into(),
        "tcp".into(),
        "-nolisten".into(),
        "local".into(),
        "-noreset".into(),
        "-nolock".into(),
    ]);
    argv
}

/// Whether `folder` is among the system's programs every sandbox reads.
fn system(folder: &Path) -> bool {
    limits()
        .system_folders
        .iter()
        .any(|root| folder.starts_with(root))
}

/// `Xvfb` in the system's folders (`limits.toml`), never on PATH: the
/// first there that, links resolved, is root's along with its folder, and
/// no one else may write either.
fn system_xvfb() -> Option<PathBuf> {
    limits()
        .xvfb_folders
        .iter()
        .map(|folder| Path::new(folder).join("Xvfb"))
        .find(|program| roots(program))
}

/// Whether `program`, its folder, and what it links to and its folder
/// are root's, a file, and written by no one else.
fn roots(program: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(real) = program.canonicalize() else {
        return false;
    };
    let root_only = |path: &Path| {
        std::fs::metadata(path).is_ok_and(|meta| meta.uid() == 0 && meta.mode() & 0o022 == 0)
    };
    std::fs::metadata(&real).is_ok_and(|meta| meta.is_file())
        && [program.parent(), Some(real.as_path()), real.parent()]
            .into_iter()
            .flatten()
            .all(root_only)
}

/// Takes the display's `lock` for this tool, as X servers take theirs:
/// made only if it isn't there, naming this process.
fn hold(lock: &Path) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o444);
    let mut file = options
        .open(lock)
        .with_context(|| format!("taking the display's lock, {}", lock.display()))?;
    writeln!(file, "{:>10}", std::process::id())
        .with_context(|| format!("writing {}", lock.display()))
}

/// Makes the folder of X's sockets, `socket`'s, as X keeps it (anyone's
/// to make a socket in, each its maker's), unless it's there. Two Xvfbs
/// that both find none race to make it, and the one that loses can't
/// listen.
fn sockets(socket: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let Some(dir) = socket.parent() else {
        return Ok(());
    };
    match std::fs::DirBuilder::new().mode(0o1777).create(dir) {
        // The mode as asked, whatever the umask took.
        Ok(()) => std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o1777))
            .with_context(|| format!("opening {} to X servers", dir.display())),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e).with_context(|| format!("making {}", dir.display())),
    }
}

/// Stops `server`, the sandbox running Xvfb, and the group it leads as
/// they let themselves be; killed if they won't within a moment.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a stopping Xvfb until it ends"
)]
fn stop(server: &mut Child) {
    let group = server.id() as libc::pid_t;
    // Never group 1 or below: `kill(-1)` is every process.
    if group > 1 {
        // SAFETY: a signal to the process group the server leads.
        unsafe { libc::kill(-group, libc::SIGTERM) };
    }
    let deadline = Instant::now() + Duration::from_millis(limits().stop_ms);
    while Instant::now() < deadline {
        if server.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(limits().stop_poll_ms));
    }
    if group > 1 {
        // SAFETY: as above.
        unsafe { libc::kill(-group, libc::SIGKILL) };
    }
    let _ = server.kill();
    let _ = server.wait();
}

/// Removes a display's `lock` and `socket` left by a server that's gone
/// (killed, or its machine stopped): its lock names a process not
/// running.
fn stale(lock: &Path, socket: &Path) {
    let Ok(text) = std::fs::read_to_string(lock) else {
        return;
    };
    let Ok(pid) = text.trim().parse::<libc::pid_t>() else {
        return;
    };
    // SAFETY: signal 0 only asks whether the process is there.
    let gone = pid > 0
        && unsafe { libc::kill(pid, 0) } == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
    if gone {
        let _ = std::fs::remove_file(socket);
        let _ = std::fs::remove_file(lock);
    }
}

/// The last `kept` bytes of what `errors` gives, read to its end.
fn keep_errors(mut errors: impl std::io::Read, kept: usize) -> Vec<u8> {
    let mut end = VecDeque::new();
    let mut buffer = [0u8; 4096];
    while let Ok(n @ 1..) = errors.read(&mut buffer) {
        end.extend(&buffer[..n]);
        let over = end.len().saturating_sub(kept);
        end.drain(..over);
    }
    end.into()
}

fn opened(conn: RustConnection<Timed>, server: Option<Server>) -> Result<Open> {
    conn.extension_information(x11rb::protocol::xtest::X11_EXTENSION_NAME)?
        .context("the display has no XTEST, which input goes through")?;
    let screen = &conn.setup().roots[0];
    let (root, width, height) = (screen.root, screen.width_in_pixels, screen.height_in_pixels);
    Ok(Open {
        _server: server,
        conn,
        root,
        width,
        height,
    })
}

#[cfg(test)]
link_test_kit::program!(system);

/// Runs `work` between `press` and `release`, released whatever `work`
/// did once pressed, so nothing stays held down after a failure.
fn held(
    press: impl FnOnce() -> Result<()>,
    work: impl FnOnce() -> Result<()>,
    release: impl FnOnce() -> Result<()>,
) -> Result<()> {
    press()?;
    let worked = work();
    let released = release();
    worked.and(released)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A button pressed is released though the work between fails, and
    /// the work's failure is what's said; one never pressed isn't released.
    #[test]
    fn a_failed_drag_lets_go() {
        let released = std::cell::Cell::new(0);
        let failed = held(
            || Ok(()),
            || anyhow::bail!("the server hung"),
            || {
                released.set(released.get() + 1);
                Ok(())
            },
        );
        assert!(failed.unwrap_err().to_string().contains("hung"));
        assert_eq!(released.get(), 1);
        let unpressed = held(
            || anyhow::bail!("refused"),
            || Ok(()),
            || {
                released.set(released.get() + 1);
                Ok(())
            },
        );
        assert!(unpressed.is_err());
        assert_eq!(released.get(), 1);
    }

    /// Xvfb starts under the sandbox it's given, reading its program's
    /// folder outside the system's and the cookie, writing X's folder of
    /// sockets and nothing else of `/tmp`, its lock left to the tool.
    #[test]
    fn xvfb_starts_in_its_sandbox() {
        let config = Config {
            display: 120,
            auth: PathBuf::from("/home/u/.lnk/data/agents/main/desktop/auth"),
            xvfb: None,
            sandbox: vec!["lnk".into(), "sandbox".into(), "run".into()],
        };
        let argv = xvfb_argv(&config, Path::new("/opt/x/bin/Xvfb"));
        let (sandbox, program) = argv.split_at(argv.iter().position(|a| a == "--").unwrap());
        assert_eq!(
            sandbox,
            [
                "lnk",
                "sandbox",
                "run",
                "--files.read",
                "/opt/x/bin",
                "--files.read",
                "/home/u/.lnk/data/agents/main/desktop/auth",
                "--commands.allow",
                "/opt/x/bin/Xvfb",
                "--commands.toolchains",
                "desktop",
                "--files.write",
                limits().sockets.as_str(),
            ]
        );
        assert_eq!(
            program[1..3],
            ["/opt/x/bin/Xvfb".to_string(), ":120".into()]
        );
        assert!(program.iter().any(|a| a == "-nolock"));
        // The system's programs are every sandbox's already.
        let argv = xvfb_argv(&config, Path::new("/usr/bin/Xvfb"));
        assert!(!argv.iter().any(|a| a == "/usr/bin"));
    }

    /// The tool holds the display's lock as an X server does, once: a
    /// second hold of it is refused.
    #[test]
    fn the_display_lock_is_held_once() {
        let dir = link_test_kit::TempDir::new("desktop-lock");
        let lock = dir.join(".X120-lock");
        hold(&lock).unwrap();
        let held = std::fs::read_to_string(&lock).unwrap();
        assert_eq!(held, format!("{:>10}\n", std::process::id()));
        assert!(hold(&lock).is_err());
    }

    /// Xvfb is taken only where root keeps it: a program another may
    /// write, or a link to one, is passed over, whoever runs the test.
    #[test]
    fn xvfb_is_taken_only_where_root_keeps_it() {
        use std::os::unix::fs::PermissionsExt;
        assert!(roots(Path::new("/bin/sh")), "the system's shell");
        let dir = link_test_kit::TempDir::new("desktop-xvfb");
        let open = dir.join("open");
        std::fs::create_dir_all(&open).unwrap();
        // Group and others may write it, as well as its owner.
        let mut anyone = std::fs::metadata(&open).unwrap().permissions();
        anyone.set_mode(anyone.mode() | 0o022);
        std::fs::set_permissions(&open, anyone).unwrap();
        let program = open.join("Xvfb");
        std::fs::write(&program, "").unwrap();
        assert!(!roots(&program));
        let linked = open.join("linked");
        std::os::unix::fs::symlink("/bin/sh", &linked).unwrap();
        assert!(!roots(&linked), "a link in a folder anyone writes");
        assert!(!roots(&dir.join("nowhere")));
        assert!(
            limits()
                .xvfb_folders
                .iter()
                .all(|folder| system(Path::new(folder)))
        );
    }

    /// Only the end of a server's errors is kept, however much it wrote.
    #[test]
    fn only_the_end_of_the_errors_is_kept() {
        let said = vec![b'a'; 10_000]
            .into_iter()
            .chain(*b"last")
            .collect::<Vec<u8>>();
        let kept = keep_errors(said.as_slice(), 8);
        assert_eq!(kept, b"aaaalast");
    }

    /// A borrowed key's keysym is kept in the call's keymap until it's
    /// given back, so two borrowed at once take two keys.
    #[test]
    fn the_keymap_follows_what_is_borrowed() {
        let mut map = Keymap {
            min: 8,
            per: 2,
            keysyms: vec![0x61, 0x41, 0, 0, 0, 0],
        };
        map.set(9, 0x0100_20ac);
        assert_eq!(map.keysyms, [0x61, 0x41, 0x0100_20ac, 0x0100_20ac, 0, 0]);
        map.set(9, 0);
        assert_eq!(map.keysyms, [0x61, 0x41, 0, 0, 0, 0]);
        // A key a program mapped since is copied as the display has it.
        map.copy(10, &[0x62]);
        assert_eq!(map.keysyms, [0x61, 0x41, 0, 0, 0x62, 0]);
        map.copy(7, &[0x63]);
        assert_eq!(map.keysyms, [0x61, 0x41, 0, 0, 0x62, 0]);
    }

    /// Without a sandbox, Xvfb isn't started.
    #[test]
    fn xvfb_never_starts_unconfined() {
        let dir = link_test_kit::TempDir::new("desktop-unconfined");
        let err = start(&Config {
            display: 0,
            auth: dir.join("auth"),
            xvfb: Some(dir.join("Xvfb")),
            sandbox: Vec::new(),
        })
        .err()
        .unwrap()
        .to_string();
        assert!(err.contains("only in a sandbox"), "{err}");
    }
}
