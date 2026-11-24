//! The agent's screens: Link browser (`lnk-browser-tool`), pages it
//! opens, reads and clicks in a browser of its own, and Link desktop
//! (`lnk-desktop-tool`), a virtual desktop it sees and works, given by
//! `lnk agent tool add browser` and `add desktop`.
//!
//! Each is a tool defined for the agent alone as `run` is
//! (`browser-<key>`, [`run_file::add`]), shared by all its sessions
//! (`--shared`), so they share one browser, each conversation with a
//! page of its own and only the owner's using its sign-ins, or one
//! desktop, and handed to its harness by its plain name. The browser
//! reaches the internet hosts, localhost ports and hosts of your network
//! the agent's sandbox file names, and no more. What each keeps
//! is runtime and sign-ins, never the agent's state, so it's in the
//! agent's folder in `~/.lnk/data/agents/<name>/`, which no harness
//! reaches: the browser's profile (`browser/`), the desktop's display
//! number and cookie (`desktop/`). Defined again at each start, so a moved program is
//! followed.

use link_format::WithinExt;
use std::path::{Path, PathBuf};

use anyhow::bail;
use link_agent::settings::Settings;

use crate::run_file;

/// The screens Link offers, by the names `lnk agent tool` gives them and
/// the agent's spec switches them by.
pub use link_agent_spec::SCREENS;

/// The display numbers desktops take (`screens.toml`), and where each
/// is reserved: your user's runtime folder, which no other user writes.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Displays {
    first_display: u32,
    displays: u32,
    #[serde(skip)]
    reserved: PathBuf,
}

impl Displays {
    /// `screens.toml`, read once, with the reservations' folder.
    fn get() -> &'static Displays {
        static READ: std::sync::OnceLock<Displays> = std::sync::OnceLock::new();
        READ.get_or_init(|| Displays {
            reserved: link_files::layout::Runtime::Displays.path(),
            ..toml::from_str(include_str!("screens.toml")).expect("screens.toml reads")
        })
    }
}

/// Screen `which`'s module.
fn module(which: &str) -> &'static str {
    match which {
        "desktop" => "desktop-tool",
        _ => "browser-tool",
    }
}

/// Where the agent's browser keeps its profile.
pub fn profile(agent: &Settings) -> PathBuf {
    agent.data(link_files::layout::AgentData::Browser)
}

/// Where the agent's desktop keeps its display's number and cookie.
pub fn desktop_dir(agent: &Settings) -> PathBuf {
    agent.data(link_files::layout::AgentData::Desktop)
}

/// The agent's desktop, while it has one: its display's number and its
/// cookie's file, which each program `run` starts is given.
pub fn desktop(agent: &Settings) -> Option<(u32, PathBuf)> {
    if !runs_here("desktop") {
        return None;
    }
    run_file::tool(agent, "desktop")?;
    let dir = desktop_dir(agent);
    let display = std::fs::read_to_string(dir.join("display"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some((display, dir.join("auth")))
}

/// Each screen the agent has, by the name its harness is handed it as
/// and the tool's own (`browser`, `browser-<key>`).
pub fn handed(agent: &Settings) -> Vec<(String, String)> {
    SCREENS
        .iter()
        .filter(|which| runs_here(which))
        .filter_map(|which| Some((which.to_string(), run_file::tool(agent, which)?)))
        .collect()
}

/// Whether screen `which` runs on this machine: the desktop, an `Xvfb`
/// display, only on Linux.
pub fn runs_here(which: &str) -> bool {
    runs_on(which, cfg!(target_os = "linux"))
}

/// Whether screen `which` runs on a machine that is Linux (`linux`), or
/// on one that isn't.
fn runs_on(which: &str, linux: bool) -> bool {
    linux || which != "desktop"
}

/// Gives the agent screen `which`, its module installed if it's missing:
/// whether it changed what the agent has.
pub fn add(agent: &Settings, which: &str) -> anyhow::Result<bool> {
    let had = run_file::tool(agent, which).is_some();
    let program = run_file::program(module(which))?;
    define(agent, which, &program)?;
    Ok(!had)
}

/// Takes screen `which` away: whether it had it.
pub fn remove(agent: &Settings, which: &str) -> bool {
    let had = run_file::tool(agent, which).is_some();
    if which == "desktop" {
        release(&Displays::get().reserved, &desktop_dir(agent));
    }
    run_file::forget(agent, which);
    had
}

/// Defines again, at a start, each screen the agent has, and the screens
/// its spec switches: `enabled = true` gives one it hasn't, `false` takes
/// one it has, and a spec that says nothing leaves what `lnk agent tool
/// add` gave. One this machine can't run (a desktop, its agent moved to a
/// Mac) is neither defined nor handed, and said: kept for a machine that
/// can.
pub fn define_all(agent: &Settings) -> anyhow::Result<()> {
    let spec = agent.spec()?;
    for which in SCREENS {
        let has = run_file::tool(agent, which).is_some();
        match at_start(spec.tool(which).enabled, has) {
            AtStart::Take => {
                remove(agent, which);
                continue;
            }
            AtStart::Leave => continue,
            AtStart::Define => {}
        }
        if !runs_here(which) {
            link_format::aside(
                link_format::Report::warning(link_format::sentence!(
                    "Your agent's desktop is off on this machine."
                ))
                .why(link_format::prose!(
                    "Its desktop is a virtual screen, made by Linux's Xvfb, and this computer \
                     isn't Linux. It's back when your agent starts on a Linux machine."
                )),
            );
            continue;
        }
        let program = run_file::program(module(which))?;
        define(agent, which, &program)?;
    }
    Ok(())
}

/// What a start does with a screen.
#[derive(Debug, PartialEq)]
enum AtStart {
    /// Defines it again, or for the first time.
    Define,
    /// Takes it away.
    Take,
    /// Leaves it, as the agent has none.
    Leave,
}

/// What a start does with a screen its spec switches as `switched`,
/// which the agent `has` or not: a spec's `true` gives it, its `false`
/// takes it, and a silent spec leaves what `lnk agent tool add` gave.
fn at_start(switched: Option<bool>, has: bool) -> AtStart {
    match (switched, has) {
        (Some(false), _) => AtStart::Take,
        (Some(true), _) | (None, true) => AtStart::Define,
        (None, false) => AtStart::Leave,
    }
}

/// Defines screen `which` for the agent.
fn define(agent: &Settings, which: &str, program: &Path) -> anyhow::Result<String> {
    let argv = match which {
        "desktop" => {
            let dir = desktop_dir(agent);
            std::fs::create_dir_all(&dir).within(format!("creating {}", dir.display()))?;
            let display = display(&dir)?;
            let auth = dir.join("auth");
            let cookie = link_desktop_tool_contract::cli::CookieCreateArgs {
                file: auth.clone(),
                display,
            };
            let made = link_secretive::process::command(program)
                .args(cookie.argv())
                .output()
                .within(format!("running {}", program.display()))?;
            if !made.status.success() {
                bail!(
                    "the desktop's cookie wasn't made: {}",
                    link_format::clean(String::from_utf8_lossy(&made.stderr).trim())
                );
            }
            // Xvfb in a sandbox of its own, as Chromium is: no network,
            // reading its program and the cookie, writing X's sockets.
            let lnk = link_core::lnk()
                .get_program()
                .to_string_lossy()
                .into_owned();
            let serve = link_desktop_tool_contract::cli::DesktopToolServeArgs {
                display,
                xauthority: auth,
                xvfb: None,
                sandbox: screen_sandbox(&lnk, None),
            };
            std::iter::once(program.display().to_string())
                .chain(serve.argv())
                .collect()
        }
        _ => {
            let lnk = link_core::lnk()
                .get_program()
                .to_string_lossy()
                .into_owned();
            let profile = profile(agent);
            std::fs::create_dir_all(&profile).within(format!("creating {}", profile.display()))?;
            let sandbox = link_agent::sandbox_file::load(&agent.name)?;
            let serve = link_browser_tool_contract::cli::BrowserToolServeArgs {
                profile,
                sandbox: screen_sandbox(&lnk, run_file::route(agent)),
                chromium: browser_program(agent.spec()?.tool("browser").program.as_deref())?,
                floor: browser_floor(agent)?,
                internet_host: internet_hosts(&sandbox),
                private_host: private_hosts(&sandbox),
            };
            std::iter::once(program.display().to_string())
                .chain(serve.argv())
                .collect()
        }
    };
    let folder = agent.folder();
    let sandbox = link_agent::sandbox_file::Sandbox::none();
    run_file::add(agent, which, program, &folder, &sandbox, argv)
}

/// The program the agent's browser starts, as its spec names it
/// (`[tool.browser] program`), checked here: none, and the browser finds
/// one. It's control, read from the spec alone, and the browser allows it
/// in its sandbox as it allows one it finds.
fn browser_program(named: Option<&str>) -> anyhow::Result<Option<PathBuf>> {
    let Some(named) = named else {
        return Ok(None);
    };
    let program = PathBuf::from(named);
    link_browser_tool_contract::program::check(&program, PROGRAM_SETTING)?;
    Ok(Some(program))
}

/// The spec's setting that names the browser's program, as a refusal
/// names it.
const PROGRAM_SETTING: &str = "[tool.browser] program";

/// The internet hosts `sandbox`, the agent's sandbox file, names on this
/// machine, which its browser reaches: none for none.
fn internet_hosts(sandbox: &link_agent::sandbox_file::Sandbox) -> Vec<String> {
    sandbox
        .spec
        .on(link_sandbox_contract::spec::Os::here())
        .unwrap_or_default()
        .internet
        .and_then(|internet| internet.hosts)
        .unwrap_or_default()
}

/// The private hosts `sandbox`, the agent's sandbox file, names on this
/// machine, which its browser reaches as it reaches its internet hosts:
/// each localhost port it names (`localhost:<port>`) and each host of
/// your network (`[lan] hosts`). Every port or every private range
/// (`*`) names no host the browser can be given, so it reaches none of
/// them: none for none.
fn private_hosts(sandbox: &link_agent::sandbox_file::Sandbox) -> Vec<String> {
    let spec = sandbox
        .spec
        .on(link_sandbox_contract::spec::Os::here())
        .unwrap_or_default();
    let ports = spec
        .localhost
        .and_then(|localhost| localhost.ports)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|port| match port {
            link_sandbox_contract::spec::Port::Number(port) => Some(format!("localhost:{port}")),
            _ => None,
        });
    let lan = spec
        .lan
        .and_then(|lan| lan.hosts)
        .unwrap_or_default()
        .into_iter()
        .filter(|host| host != "*");
    ports.chain(lan).collect()
}

/// The agent's floor as its browser takes it: the standing of a call
/// that says none, from its spec and teams ([`crate::scheduler::floor`]).
fn browser_floor(
    agent: &Settings,
) -> anyhow::Result<link_browser_tool_contract::cli::BrowserToolServeArgsFloor> {
    use link_browser_tool_contract::cli::BrowserToolServeArgsFloor as Floor;
    use link_scheduler_tool_contract::cli::FlagsFloor;
    Ok(match crate::scheduler::floor(agent)?.floor {
        Some(FlagsFloor::Owner) => Floor::Owner,
        _ => Floor::Member,
    })
}

/// The sandbox a screen starts its program in, after `serve`'s `--`: the
/// browser's Chromium, out through the agent's route (`upstream`) when it
/// has one, as `run`'s calls go; the desktop's Xvfb, with no way out.
fn screen_sandbox(lnk: &str, upstream: Option<&str>) -> Vec<String> {
    // What it runs, the screen's program names itself.
    let mut sandbox = vec![lnk.into(), "sandbox".into(), "run".into()];
    if let Some(upstream) = upstream {
        sandbox.extend(["--upstream".into(), upstream.into()]);
    }
    sandbox
}

/// The desktop's display number, kept in `dir` and reserved across the
/// machine ([`reserve`]): picked once, the first no X server here holds
/// and no other desktop reserved.
fn display(dir: &Path) -> anyhow::Result<u32> {
    let displays = Displays::get();
    reservations(&displays.reserved)?;
    display_in(dir, displays, |n| {
        Path::new(&format!("/tmp/.X{n}-lock")).exists()
            || Path::new(&format!("/tmp/.X11-unix/X{n}")).exists()
    })
}

/// [`display`], with `displays`' numbers and reservations, a number held
/// by an X server when `taken`. A number kept but reserved by another
/// desktop since (its reservation lost, as at a restart, and taken) is
/// given up for a new one, and the cookie made for it with it.
fn display_in(dir: &Path, displays: &Displays, taken: impl Fn(u32) -> bool) -> anyhow::Result<u32> {
    let kept = dir.join("display");
    if let Some(n) = std::fs::read_to_string(&kept)
        .ok()
        .and_then(|n| n.trim().parse().ok())
    {
        if reserve(&displays.reserved, n, dir)? {
            return Ok(n);
        }
        // Its cookie names the number it was made for.
        match std::fs::remove_file(dir.join("auth")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(e).within(format!("removing {}", dir.join("auth").display()));
            }
            _ => {}
        }
    }
    let mut picked = None;
    for n in displays.first_display..displays.first_display + displays.displays {
        if !taken(n) && reserve(&displays.reserved, n, dir)? {
            picked = Some(n);
            break;
        }
    }
    let n = picked.within("no display number is free here")?;
    link_files::write_private(&kept, format!("{n}\n").as_bytes())
        .within(format!("writing {}", kept.display()))?;
    Ok(n)
}

/// Makes the folder of reservations, `folder`, your user's alone: no
/// other user can fill it with reservations this one can't judge.
fn reservations(folder: &Path) -> anyhow::Result<()> {
    link_files::create_private_dir(folder).within(format!("making {}", folder.display()))
}

/// Reserves display `n` in `folder` for the desktop kept in `dir`, the
/// file `<n>` naming it: whether it's this desktop's now. Another's
/// reservation stands unless it's gone stale: its desktop's folder gone,
/// or keeping another number. One whose desktop can't be read stands.
/// Another user's desktops reserve in their own folder: their X servers
/// are passed over when a number is picked, and one that took a number
/// first holds it by X's own lock.
fn reserve(folder: &Path, n: u32, dir: &Path) -> anyhow::Result<bool> {
    let file = folder.join(n.to_string());
    let holder = dir.display().to_string();
    for _ in 0..2 {
        match std::fs::File::create_new(&file) {
            Ok(mut made) => {
                std::io::Write::write_all(&mut made, holder.as_bytes())
                    .within(format!("writing {}", file.display()))?;
                return Ok(true);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e).within(format!("reserving {}", file.display())),
        }
        let held = match std::fs::read_to_string(&file) {
            Ok(held) => held,
            // Released meanwhile: made again.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Ok(false),
        };
        if held == holder {
            return Ok(true);
        }
        if !stale(Path::new(&held), n) || std::fs::remove_file(&file).is_err() {
            return Ok(false);
        }
    }
    Ok(false)
}

/// Whether the reservation of display `n` by the desktop kept in
/// `holder` is stale: that folder gone, or keeping another number. One
/// not yet keeping its number (picking it now) or not readable stands.
fn stale(holder: &Path, n: u32) -> bool {
    if !holder.is_absolute() {
        return false;
    }
    match std::fs::read_to_string(holder.join("display")) {
        Ok(kept) => kept.trim().parse::<u32>().is_ok_and(|kept| kept != n),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            matches!(holder.try_exists(), Ok(false))
        }
        Err(_) => false,
    }
}

/// Gives up the desktop kept in `dir`'s reservation of its display, as
/// it's taken away.
fn release(folder: &Path, dir: &Path) {
    let Some(n) = std::fs::read_to_string(dir.join("display"))
        .ok()
        .and_then(|n| n.trim().parse::<u32>().ok())
    else {
        return;
    };
    let file = folder.join(n.to_string());
    if std::fs::read_to_string(&file).is_ok_and(|held| held == dir.display().to_string()) {
        let _ = std::fs::remove_file(&file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spec's `true` gives a screen and its `false` takes one; a silent
    /// spec keeps what the agent has, and gives none it hasn't.
    #[test]
    fn a_spec_switches_a_screen_and_a_silent_one_keeps_what_was_added() {
        for has in [true, false] {
            assert_eq!(at_start(Some(true), has), AtStart::Define);
            assert_eq!(at_start(Some(false), has), AtStart::Take);
        }
        assert_eq!(at_start(None, true), AtStart::Define);
        assert_eq!(at_start(None, false), AtStart::Leave);
    }

    /// Displays from `first`, reserved in a folder of this test's.
    fn displays(root: &Path, first: u32) -> Displays {
        let reserved = root.join("reserved");
        std::fs::create_dir_all(&reserved).unwrap();
        Displays {
            first_display: first,
            displays: 10,
            reserved,
        }
    }

    /// A desktop's own folder in `root`, named `name`.
    fn desktop_in(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name).join("desktop");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An idle desktop's number is reserved across the machine: another
    /// desktop picking while no X server holds it takes the next, and the
    /// first keeps its own. An X server's number is passed over too.
    #[test]
    fn an_idle_desktops_number_goes_to_no_other() {
        let root = link_test_kit::TempDir::new("displays-idle");
        let displays = displays(&root, 100);
        let (a, b) = (desktop_in(&root, "a"), desktop_in(&root, "b"));
        let nothing_runs = |_: u32| false;
        assert_eq!(display_in(&a, &displays, nothing_runs).unwrap(), 100);
        assert_eq!(display_in(&b, &displays, nothing_runs).unwrap(), 101);
        assert_eq!(display_in(&a, &displays, nothing_runs).unwrap(), 100);
        assert_eq!(display_in(&b, &displays, nothing_runs).unwrap(), 101);
        let c = desktop_in(&root, "c");
        assert_eq!(display_in(&c, &displays, |n| n == 102).unwrap(), 103);
    }

    /// A kept number another desktop reserved since (the reservations
    /// lost at a restart) is given up for a new one, with the cookie made
    /// for the old; a reservation gone stale (its desktop deleted, or
    /// keeping another number) is taken, and one given up is free.
    #[test]
    fn a_desktop_never_keeps_a_number_another_holds() {
        let root = link_test_kit::TempDir::new("displays-kept");
        let displays = displays(&root, 200);
        let (a, b) = (desktop_in(&root, "a"), desktop_in(&root, "b"));
        assert_eq!(display_in(&a, &displays, |_| false).unwrap(), 200);
        // A restart: no reservation, and the other desktop picks first.
        std::fs::remove_file(displays.reserved.join("200")).unwrap();
        assert_eq!(display_in(&b, &displays, |_| false).unwrap(), 200);
        std::fs::write(a.join("auth"), "cookie for 200").unwrap();
        assert_eq!(display_in(&a, &displays, |_| false).unwrap(), 201);
        assert!(!a.join("auth").exists());
        // Its holder deleted: stale, taken.
        std::fs::remove_dir_all(b.parent().unwrap()).unwrap();
        let c = desktop_in(&root, "c");
        assert_eq!(display_in(&c, &displays, |_| false).unwrap(), 200);
        // Its holder keeping another number: stale, taken.
        std::fs::write(c.join("display"), "205\n").unwrap();
        let d = desktop_in(&root, "d");
        assert_eq!(display_in(&d, &displays, |_| false).unwrap(), 200);
        // Given up: free for the next.
        release(&displays.reserved, &d);
        assert!(!displays.reserved.join("200").exists());
        let e = desktop_in(&root, "e");
        assert_eq!(display_in(&e, &displays, |_| false).unwrap(), 200);
    }

    /// The machine's display numbers and reservations read.
    #[test]
    fn the_display_numbers_read() {
        let displays = Displays::get();
        assert!(displays.first_display > 0 && displays.displays > 0);
        assert!(displays.reserved.is_absolute());
        assert!(
            displays
                .reserved
                .starts_with(link_files::layout::Root::Runtime.path())
        );
    }

    /// The folder of reservations is made your user's alone, whatever
    /// mode it was left with.
    #[test]
    fn reservations_are_your_users_alone() {
        use std::os::unix::fs::PermissionsExt;
        let root = link_test_kit::TempDir::new("displays-private");
        let folder = root.join("reserved");
        std::fs::create_dir(&folder).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o1777)).unwrap();
        reservations(&folder).unwrap();
        let mode = std::fs::metadata(&folder).unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o700);
    }

    /// The desktop runs only on Linux, wherever it's started from: a
    /// start on a Mac neither defines nor hands it. The browser runs on
    /// either.
    #[test]
    fn the_desktop_runs_only_on_linux() {
        assert!(runs_on("desktop", true));
        assert!(!runs_on("desktop", false));
        assert!(runs_on("browser", true));
        assert!(runs_on("browser", false));
        assert_eq!(runs_here("desktop"), cfg!(target_os = "linux"));
        assert!(runs_here("browser"));
    }

    /// The browser's sandbox leaves through the agent's route when it has
    /// one, as `run`'s calls do, and as the sandbox does by default without.
    #[test]
    fn the_browser_leaves_through_the_agents_route() {
        let routed = screen_sandbox("lnk", Some("http://127.0.0.1:3128"));
        assert_eq!(
            routed,
            [
                "lnk",
                "sandbox",
                "run",
                "--upstream",
                "http://127.0.0.1:3128"
            ]
        );
        let direct = screen_sandbox("lnk", None);
        assert_eq!(direct, ["lnk", "sandbox", "run"]);
    }

    /// A program the spec names is the one the browser is given, checked
    /// first and refused by the setting's name; none, and the browser
    /// finds one as it always has.
    #[test]
    fn the_specs_program_is_the_browsers() {
        assert_eq!(browser_program(None).unwrap(), None);
        let dir = link_test_kit::TempDir::new("browser-program");
        let program = dir.join("chrome");
        link_test_kit::write_program(&program, b"\x7fELF\x02\x01\x01").unwrap();
        let named = program.display().to_string();
        assert_eq!(browser_program(Some(&named)).unwrap(), Some(program));
        let script = dir.join("launcher");
        link_test_kit::write_program(&script, b"#!/bin/sh\nexec chrome\n").unwrap();
        let script = script.display().to_string();
        let missing = dir.join("missing").display().to_string();
        for bad in [script.as_str(), "chrome", missing.as_str()] {
            let refused = format!("{:#}", browser_program(Some(bad)).unwrap_err());
            assert!(refused.starts_with(PROGRAM_SETTING), "{refused}");
        }
    }

    /// The browser reaches the internet hosts the agent's sandbox file
    /// names, every host only when it names `*`, and none when it names
    /// none.
    #[test]
    fn the_browser_reaches_the_agents_internet_hosts() {
        let hosts = |file: &str| {
            internet_hosts(
                &link_agent::sandbox_file::Sandbox::parse(&format!("schema = 1\n{file}")).unwrap(),
            )
        };
        assert_eq!(
            hosts("[internet]\nhosts = [\"api.example.com\", \"*.wikipedia.org\"]\n"),
            ["api.example.com", "*.wikipedia.org"]
        );
        assert_eq!(hosts("[internet]\nhosts = [\"*\"]\n"), ["*"]);
        assert!(hosts("").is_empty());
        assert!(internet_hosts(&link_agent::sandbox_file::Sandbox::none()).is_empty());
    }

    /// The browser reaches the localhost ports and the hosts of your
    /// network the agent's sandbox file names, each by name, and none
    /// when it names none or only every one (`*`).
    #[test]
    fn the_browser_reaches_the_agents_private_hosts() {
        let hosts = |file: &str| {
            private_hosts(
                &link_agent::sandbox_file::Sandbox::parse(&format!("schema = 1\n{file}")).unwrap(),
            )
        };
        assert_eq!(
            hosts("[localhost]\nports = [8080, 34767]\n[lan]\nhosts = [\"wiki.lan\"]\n"),
            ["localhost:8080", "localhost:34767", "wiki.lan"]
        );
        assert!(hosts("[lan]\nhosts = [\"*\"]\n").is_empty());
        assert!(hosts("[internet]\nhosts = [\"*\"]\n").is_empty());
        assert!(private_hosts(&link_agent::sandbox_file::Sandbox::none()).is_empty());
    }
}
