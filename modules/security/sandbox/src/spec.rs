//! A sandbox's spec: a TOML file whose sections are its only grants
//! (`docs/config/specs/sandbox.md`). A section left out stays closed,
//! `[commands]` too: a spec with no sections reads none of your files,
//! reaches no network and runs no program.
//!
//! [`Spec`] is what the file says and [`Confinement`] what it opens on
//! this OS, both in `link_sandbox_client`, which makes one from the
//! other. What only this machine's sandbox can judge is here: a write
//! that leads out of it ([`check`]), a program it could replace
//! ([`programs`]), and what it opens said to you ([`describe`]) and
//! remembered ([`opens`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure};
use link_sandbox_client::{Confinement, Net, Programs, SYSTEM_FOLDERS, socket_folders};

mod resolved;
pub use resolved::Resolved;

pub use link_sandbox_contract::spec::{
    Browser, Commands, Files, Internet, Lan, Localhost, Network, NetworkMode, Os, Port, Spec, Tools,
};

/// What a confinement's network opens, as its spec's sections say it.
#[derive(Default)]
struct Sections {
    /// `[internet] hosts`: None, closed; empty, any public host.
    internet: Option<Vec<String>>,
    tcp: Vec<String>,
    per_minute: Option<u32>,
    lan: Vec<String>,
    ports: BTreeSet<u16>,
    models: BTreeSet<u16>,
    tools: Vec<String>,
    /// `[browser] hosts`: None, closed; empty, any.
    browser: Option<Vec<String>>,
    /// `[network] mode = "host"`, and whether every port of the loopback.
    host: Option<bool>,
}

fn sections(c: &Confinement) -> Sections {
    match &c.net {
        Net::Closed => Sections::default(),
        Net::Loopback { ports, .. } | Net::Reaching { ports, .. } => Sections {
            ports: ports.clone(),
            ..Default::default()
        },
        Net::Host { ports, localhost } => Sections {
            ports: ports.clone(),
            host: Some(*localhost),
            ..Default::default()
        },
        Net::Proxied { proxy, ports, .. } => Sections {
            internet: (!proxy.no_internet).then(|| proxy.hosts.clone()),
            tcp: proxy.tcp.clone(),
            per_minute: proxy.per_minute,
            lan: proxy.lan.clone(),
            ports: ports.clone(),
            models: proxy.models(),
            tools: proxy.tools(),
            browser: proxy.pages().map(|hosts| match hosts.as_slice() {
                [any] if any == "*" => Vec::new(),
                hosts => hosts.to_vec(),
            }),
            host: None,
        },
    }
}

/// What on this machine no spec opens, and where a write would lead out
/// of the sandbox: what [`check`] and [`given_write`] judge against.
pub struct Machine<'a> {
    pub home: &'a Path,
    /// The machine's Hidden root.
    pub settings: &'a Path,
    /// Agents' folders (`link_sandbox_contract::hidden_dirs`).
    pub hidden: &'a [PathBuf],
    /// The spec's own file, when it's from one.
    pub spec: Option<&'a Path>,
    /// The run's `PATH`.
    pub run_path: &'a std::ffi::OsStr,
}

impl Machine<'_> {
    /// Link's settings and agents' folders, resolved: those in `~/Link`,
    /// and in Link's data part each [`things_data`] folder, with all it
    /// holds.
    fn closed(&self) -> anyhow::Result<Vec<PathBuf>> {
        let link = link_files::layout::link_in(self.home, self.settings)?;
        let configuration = link_files::layout::Home::Safe.at(&link);
        let safe = self
            .settings
            .join(&link_files::layout::LAYOUT.boundaries.hidden_safe);
        let data = things_data(&link, self.settings)?;
        Ok(std::iter::once(safe.as_path())
            .chain(std::iter::once(configuration.as_path()))
            .chain(data.iter().map(PathBuf::as_path))
            .chain(self.hidden.iter().map(PathBuf::as_path))
            .chain(
                link_files::layout::system_safes()
                    .iter()
                    .map(PathBuf::as_path),
            )
            .flat_map(|path| [path.to_path_buf(), link_sandbox_confine::resolve(path)])
            .collect())
    }
}

/// The data part's folder of each owner whose things have a kind, an
/// archive's word for one: one thing's data is its own, so no spec, which
/// any thing may be given, opens it, at any depth.
fn things_data(link: &Path, hidden: &Path) -> anyhow::Result<Vec<PathBuf>> {
    use link_files::layout::{Part, owner_folder, owners, part_root};
    owners()
        .iter()
        .filter(|(_, owner)| owner.kind.is_some() && owner.parts.contains(&Part::Data))
        .map(|(word, _)| {
            Ok(part_root(Part::Data, link, hidden).join(owner_folder(word, Part::Data)?))
        })
        .collect()
}

/// An error if `c`, from a spec, reaches what no spec may: Link's settings
/// and agents' folders (`hidden`, `link_sandbox_contract::hidden_dirs`), a write
/// that leads out of the sandbox ([`written`]), a folder of other
/// programs' sockets ([`socket_folders`]), or the spec itself (when it's
/// from a file).
pub fn check(c: &Confinement, machine: &Machine) -> anyhow::Result<()> {
    let closed = machine.closed()?;
    let runtime = link_files::layout::Root::Runtime.try_path()?;
    for path in c.read.iter().chain(&c.write) {
        let at = link_sandbox_confine::resolve(path);
        ensure!(
            !path.starts_with(&runtime) && !at.starts_with(link_sandbox_confine::resolve(&runtime)),
            "{}: runtime sockets are handed individually, never granted from a spec",
            path.display()
        );
    }
    for path in c.read.iter().chain(&c.hide) {
        inside_closed(path, &closed)?;
    }
    for path in &c.write {
        written(path, machine)?;
    }
    for path in &c.read {
        sockets(path)?;
    }
    Ok(())
}

/// An error unless `folder`, given a run beside its spec's writes
/// (`RunWith::write`, `--with-fd`), is one a spec could write
/// ([`written`]).
pub fn given_write(folder: &Path, machine: &Machine) -> anyhow::Result<()> {
    use std::path::Component;
    ensure!(
        folder.is_absolute()
            && folder
                .components()
                .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
            && folder.is_dir(),
        "{} isn't a folder, in full, to write",
        folder.display()
    );
    written(folder, machine)
}

/// An error if `path`, as written or resolved, is inside Link's settings
/// or an agent's folder (`closed`, resolved).
fn inside_closed(path: &Path, closed: &[PathBuf]) -> anyhow::Result<()> {
    let at = link_sandbox_confine::resolve(path);
    for closed in closed {
        ensure!(
            !path.starts_with(closed) && !at.starts_with(closed),
            "{}: Link's settings and agents' folders are closed to every spec",
            path.display()
        );
    }
    Ok(())
}

/// An error if writing `path` leads out of the sandbox: inside or holding
/// Link's settings or an agent's folder, in a project's dot files, your
/// home itself, a dot folder in it, `~/bin`, `~/Library`, the system's, a
/// folder on the run's `PATH`, one of other programs' sockets, or holding
/// the spec itself.
fn written(path: &Path, machine: &Machine) -> anyhow::Result<()> {
    let real = link_sandbox_confine::resolve;
    let mut closed = machine.closed()?;
    let programs = machine
        .settings
        .join(&link_files::layout::LAYOUT.boundaries.exec);
    closed.extend([programs.clone(), real(&programs)]);
    let system_programs = link_files::layout::SystemRoot::Exec.path();
    closed.extend([system_programs.clone(), real(&system_programs)]);
    inside_closed(path, &closed)?;
    let at = real(path);
    // The literal snapshot stays protected even if its name now resolves
    // elsewhere. A later wrap uses that snapshot, never the new target.
    let places = [path, at.as_path()];
    // In a project's `.git`, `.vscode`, ...: what runs outside.
    let inside = path
        .components()
        .chain(at.components())
        .find(|part| link_sandbox_confine::dotfiles::named(&part.as_os_str().to_string_lossy()));
    if let Some(name) = inside {
        bail!(
            "{}: in a project's {}, which runs outside the sandbox; write the project's \
             folder, and name it in write_dot_files",
            path.display(),
            name.as_os_str().to_string_lossy()
        );
    }
    let home_real = real(machine.home);
    ensure!(
        places.iter().all(|at| **at != home_real),
        "{}: your home folder itself can't be written; name the folders in it",
        path.display()
    );
    for closed in &closed {
        ensure!(
            places.iter().all(|at| !closed.starts_with(at)),
            "{}: it holds Link's settings or agents' folders, which no spec writes",
            path.display()
        );
    }
    for at in places {
        let Ok(rest) = at.strip_prefix(&home_real) else {
            continue;
        };
        let first = rest.components().next();
        let first = first
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default();
        ensure!(
            !first.starts_with('.')
                || at.starts_with(
                    machine
                        .settings
                        .join(&link_files::layout::LAYOUT.boundaries.data)
                ),
            "{}: a dot folder in your home (~/{first}) holds what runs outside the \
             sandbox (a shell's startup files, SSH's keys); no spec writes it",
            path.display()
        );
        ensure!(
            first != "Library",
            "{}: ~/Library holds what macOS runs at login; no spec writes it",
            path.display()
        );
        ensure!(
            first != "bin",
            "{}: ~/bin holds programs you run outside the sandbox; no spec writes it",
            path.display()
        );
    }
    ensure!(!places.contains(&Path::new("/")), "/ can't be written");
    for system in SYSTEM_FOLDERS {
        ensure!(
            places.iter().all(|at| !at.starts_with(system)),
            "{}: the system's folders can't be written",
            path.display()
        );
    }
    // What your shell finds on PATH runs outside, as you.
    for dir in std::env::split_paths(machine.run_path).filter(|d| d.is_absolute()) {
        let dir = real(&dir);
        ensure!(
            places
                .iter()
                .all(|at| !at.starts_with(&dir) && !dir.starts_with(at)),
            "{}: {} is on your PATH, so what's written there runs outside the \
             sandbox; no spec writes it",
            path.display(),
            dir.display()
        );
    }
    sockets(path)?;
    if let Some(spec) = machine.spec {
        let spec = real(spec);
        ensure!(
            places.iter().all(|at| !spec.starts_with(at)),
            "{} is in a folder it lets the sandbox write ({}), so the program could \
             change its own next run: keep the spec elsewhere",
            spec.display(),
            path.display()
        );
    }
    Ok(())
}

/// An error if `path` is in a folder of other programs' sockets, or holds
/// one, as written or resolved: `/` holds `/run`.
fn sockets(path: &Path) -> anyhow::Result<()> {
    let overlap = |a: &Path, b: &Path| a.starts_with(b) || b.starts_with(a);
    let at = link_sandbox_confine::resolve(path);
    for folder in socket_folders() {
        ensure!(
            !overlap(path, &folder) && !overlap(&at, &link_sandbox_confine::resolve(&folder)),
            "{}: {} holds other programs' sockets (a session's bus, Docker's), which \
             run what they're sent outside the sandbox; no spec reads or writes it",
            path.display(),
            folder.display()
        );
    }
    Ok(())
}

/// `c`'s programs resolved through links. One in a folder it may write,
/// or in `temp` ([`link_sandbox_confine::programs::temp_folders`]), is refused: the program would write
/// the program it runs. So is a folder holding one of those. A
/// build folder is the one exception ([`Confinement::builds`]): what a
/// build makes there (build scripts, tests), or an install unpacks, runs
/// by design. None: any program.
pub fn programs(c: &Confinement, temp: &[PathBuf]) -> anyhow::Result<Programs> {
    let Programs::Only(allowed) = &c.commands else {
        return Ok(Programs::Any);
    };
    let mut programs = Vec::new();
    for entry in allowed {
        if c.builds.contains(entry) {
            programs.push(link_sandbox_confine::resolve(entry));
            continue;
        }
        let program = link_sandbox_confine::resolve(entry);
        // A folder, or what may become one: only a file is judged as one.
        let folder = !program.is_file();
        for written in c
            .write
            .iter()
            .flat_map(|w| [w.clone(), link_sandbox_confine::resolve(w)])
            .chain(temp.iter().cloned())
        {
            ensure!(
                !program.starts_with(&written),
                "[commands] {} is {}, in {}, which the sandbox may write: it could replace it",
                entry.display(),
                program.display(),
                written.display()
            );
            ensure!(
                !(folder && written.starts_with(&program)),
                "[commands] {} is {}, which holds {}, a folder the sandbox may write: it \
                 could put a program there",
                entry.display(),
                program.display(),
                written.display()
            );
        }
        programs.push(program);
    }
    Ok(Programs::Only(programs))
}

/// What `c` opens, as Link remembers a spec it ran
/// ([`link_spec::Seen`]): one line per grant, every path resolved
/// through links and `programs` (from [`programs`]) as found, so the same
/// text opening something else is asked again.
pub fn opens(c: &Confinement, programs: Option<&[PathBuf]>) -> String {
    opens_at(c, programs, false)
}

fn opens_at(c: &Confinement, programs: Option<&[PathBuf]>, resolved: bool) -> String {
    let real = |v: &[PathBuf]| -> Vec<String> {
        v.iter()
            .map(|p| {
                if resolved {
                    p.display().to_string()
                } else {
                    link_sandbox_confine::resolve(p).display().to_string()
                }
            })
            .collect()
    };
    let s = sections(c);
    let mut lines = vec![
        format!("write {:?}", real(&c.write)),
        format!("read {:?}", real(&c.read)),
        format!("hide {:?}", real(&c.hide)),
        format!("read_home {}", c.read_home),
        format!("internet {:?}", s.internet),
        format!("tcp {:?}", s.tcp),
        format!("per_minute {:?}", s.per_minute),
        format!("lan {:?}", s.lan),
        format!("ports {:?}", s.ports),
        format!("models {:?}", s.models),
        format!("tools {:?}", s.tools),
        format!("browser {:?}", s.browser),
    ];
    // Each only when it has some: a spec run before without them is the
    // same spec still.
    if !c.write_dot_files.is_empty() {
        lines.push(format!("write_dot_files {:?}", real(&c.write_dot_files)));
    }
    if let Some(every_port) = s.host {
        lines.push(format!("network host, every port {every_port}"));
    }
    if c.developer_tools {
        lines.push("developer_tools true".into());
    }
    lines.push(match programs {
        None => "commands any".into(),
        Some(programs) => format!("commands {:?}", real(programs)),
    });
    lines.join("\n")
}

/// What `c` opens, one line each, for the question before a spec not run
/// before: each path as written, and where it really is when a link makes
/// that elsewhere.
pub fn describe(c: &Confinement) -> Vec<String> {
    describe_at(c, false)
}

fn describe_at(c: &Confinement, resolved: bool) -> Vec<String> {
    let list = |v: &[String], any: &str| match v.is_empty() {
        true => any.to_string(),
        false => v.join(", "),
    };
    let shown = |p: &PathBuf| {
        if resolved {
            return p.display().to_string();
        }
        let real = link_sandbox_confine::resolve(p);
        match real == *p {
            true => p.display().to_string(),
            false => format!("{} (really {})", p.display(), real.display()),
        }
    };
    let paths = |v: &[PathBuf]| v.iter().map(shown).collect::<Vec<_>>().join(", ");
    let s = sections(c);
    let mut lines = Vec::new();
    if !c.write.is_empty() {
        lines.push(format!("writes {}", paths(&c.write)));
    }
    if !c.write_dot_files.is_empty() {
        lines.push(format!(
            "writes what runs outside it in {} too: git's settings and hooks, .envrc, \
             .vscode, .idea",
            paths(&c.write_dot_files)
        ));
    }
    if c.read_home {
        lines.push("reads your whole home folder".into());
    }
    if !c.read.is_empty() {
        lines.push(format!("reads {}", paths(&c.read)));
    }
    if let Some(every_port) = s.host {
        lines.push(format!(
            "reaches the network as this machine does, with no proxy: the internet, your \
             network and {}, unfiltered and unlogged",
            // Linux shares this machine's loopback whole.
            match every_port || cfg!(target_os = "linux") {
                true => "every port of this machine".to_string(),
                false => "this machine's ports below".to_string(),
            }
        ));
    }
    if let Some(hosts) = &s.internet {
        lines.push(format!(
            "reaches on the internet: {}",
            list(hosts, "any public host")
        ));
    }
    if !s.tcp.is_empty() {
        lines.push(format!("reaches raw: {}", s.tcp.join(", ")));
    }
    if !s.lan.is_empty() {
        lines.push(format!("reaches your network: {}", s.lan.join(", ")));
    }
    if !s.ports.is_empty() {
        let ports: Vec<_> = s.ports.iter().map(u16::to_string).collect();
        lines.push(format!("reaches this machine's ports {}", ports.join(", ")));
    }
    if !s.models.is_empty() {
        let ports: Vec<_> = s.models.iter().map(u16::to_string).collect();
        lines.push(format!("reaches the models on ports {}", ports.join(", ")));
    }
    if !s.tools.is_empty() {
        lines.push(format!("calls your tools {}, as you", s.tools.join(", ")));
    }
    if let Some(hosts) = &s.browser {
        lines.push(format!(
            "asks you to open pages on: {}",
            list(hosts, "any host")
        ));
    }
    if c.developer_tools {
        lines.push("runs Xcode's developer tools, read only".into());
    }
    match &c.commands {
        Programs::Any => lines.push("runs any program".into()),
        Programs::Only(c) if c.is_empty() => lines.push("runs no program".into()),
        Programs::Only(c) => lines.push(format!("runs {}", paths(c))),
    }
    lines
}

pub use link_sandbox_proxy::lan::{lan_here, on_cloud};

/// An error if `c` reaches a cloud's own network on a cloud's machine
/// (`cloud`): `[network] mode = "host"`, which reaches all of it and its
/// metadata service, which hands out the machine's credentials, or a
/// `[lan]` holding a whole private block ([`lan_here`]).
pub fn network_here(c: &Confinement, cloud: bool) -> anyhow::Result<()> {
    ensure!(
        !(cloud && matches!(c.net, Net::Host { .. })),
        "[network] mode = \"host\" isn't taken on a cloud's machine: it would reach the cloud's \
         network and its metadata service, which hands out this machine's credentials; list \
         the addresses in [lan] instead"
    );
    lan_here(c.lan(), cloud)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confined(text: &str) -> anyhow::Result<Confinement> {
        let spec: Spec = link_spec::parse_spec(&format!("schema = 1\n{text}"))?;
        Confinement::of(
            &spec,
            link_sandbox_client::At {
                name: "x",
                dir: Path::new("/specs"),
                home: Path::new("/home/me"),
                path: Some(&std::ffi::OsString::from("/usr/bin:/bin")),
                way: link_sandbox_client::Way::Proxy(Default::default()),
            },
        )
    }

    /// A confinement writing `write` and running `commands`, already found.
    fn running(write: &[PathBuf], commands: &[PathBuf]) -> Confinement {
        let mut c = confined("").unwrap();
        c.write = write.to_vec();
        c.commands = Programs::Only(commands.to_vec());
        c
    }

    #[test]
    fn writes_that_lead_out_and_links_folders_are_refused() {
        let (home, settings) = (Path::new("/home/me"), Path::new("/home/me/.lnk"));
        let hidden = [
            PathBuf::from("/home/me/Link/Agents"),
            PathBuf::from("/home/me/Link/Archive"),
        ];
        let path = std::ffi::OsString::from("/usr/bin:/home/me/go/bin:relative/bin");
        let machine = Machine {
            home,
            settings,
            hidden: &hidden,
            spec: Some(Path::new("/specs/app.toml")),
            run_path: &path,
        };
        let check = |text: &str| confined(text).and_then(|c| check(&c, &machine));
        for (text, why) in [
            ("[files]\nwrite = [\"~\"]", "home folder itself"),
            ("[files]\nwrite = [\"~/.ssh\"]", "dot folder"),
            ("[files]\nwrite = [\"~/.config/x\"]", "dot folder"),
            ("[files]\nwrite = [\"~/Library/LaunchAgents\"]", "Library"),
            ("[files]\nwrite = [\"/etc\"]", "holds Link's settings"),
            ("[files]\nwrite = [\"/opt/homebrew/bin\"]", "system's"),
            ("[files]\nwrite = [\"/nix/store\"]", "system's"),
            ("[files]\nwrite = [\"~/bin\"]", "~/bin"),
            ("[files]\nwrite = [\"~/go\"]", "on your PATH"),
            ("[files]\nwrite = [\"~/go/bin/x\"]", "on your PATH"),
            ("[files]\nwrite = [\"/\"]", "holds Link's settings"),
            ("[files]\nwrite = [\"~/Link\"]", "holds Link's settings"),
            (
                "[files]\nread = [\"~/.lnk/safe/sandboxes\"]",
                "closed to every spec",
            ),
            (
                "[files]\nread = [\"~/Link/Safe/Agents/main\"]",
                "closed to every spec",
            ),
            (
                "[files]\nwrite = [\"~/.lnk/exec/harnesses/claude\"]",
                "closed to every spec",
            ),
            (
                "[files]\nread = [\"~/Link/Agents/main\"]",
                "closed to every spec",
            ),
            (
                "[files]\nread = [\"~/Link/Archive/Agents\"]",
                "closed to every spec",
            ),
            ("[files]\nread = [\"/\"]", "sockets"),
            ("[files]\nread = [\"/run\"]", "sockets"),
            ("[files]\nread = [\"/run/docker.sock\"]", "sockets"),
            ("[files]\nread = [\"/var\"]", "sockets"),
            ("[files]\nwrite = [\"/tmp\"]", "sockets"),
            ("[files]\nwrite = [\"/tmp/tmux-1000\"]", "sockets"),
            ("[files]\nread = [\"/private/var/folders\"]", "sockets"),
            ("[files]\nwrite = [\".\"]", "change its own next run"),
            ("[files]\nread = [\"../x\"]", ".."),
        ] {
            let err = check(text).unwrap_err().to_string();
            assert!(err.contains(why), "{text}: {err}");
        }
        // Each thing's data folder is closed at any depth, read or written.
        let link = link_files::layout::link_in(home, settings).unwrap();
        let data = things_data(&link, settings).unwrap();
        assert!(data.len() >= 2, "{data:?}");
        for folder in &data {
            for (below, access) in [
                ("", "read"),
                ("main/browser", "read"),
                ("main/home/x", "write"),
            ] {
                let path = folder.join(below).display().to_string();
                let text = format!("[files]\n{access} = [{path:?}]");
                let err = check(&text).unwrap_err().to_string();
                assert!(err.contains("closed to every spec"), "{text}: {err}");
            }
        }
        check("[files]\nwrite = [\"~/code/app\"]\nread = [\"~\", \"~/.config/gh\"]").unwrap();
        check(
            "[files]\nwrite = [\"~/.lnk/data/context\"]\nread = [\"~/.lnk/exec/harnesses/claude\"]",
        )
        .unwrap();
    }

    #[test]
    fn programs_are_resolved_and_never_where_it_writes() {
        let root = link_test_kit::TempDir::new("spec-programs");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("tool"), "").unwrap();
        let listed = [bin.join("tool"), bin.clone()];
        let c = running(&[], &listed);
        // The temp folders are the test's own, wherever it runs.
        let found = programs(&c, &[]).unwrap().listed().unwrap();
        let bin = link_sandbox_confine::resolve(&bin);
        assert_eq!(found, [bin.join("tool"), bin.clone()]);
        assert!(link_sandbox_confine::programs::listed(
            &found,
            &bin.join("other")
        ));
        assert!(!link_sandbox_confine::programs::listed(
            &found,
            Path::new("/usr/bin/sh")
        ));
        // In a folder it writes, or a temp folder: refused.
        assert!(programs(&running(&[root.to_path_buf()], &listed), &[]).is_err());
        let temp = [link_sandbox_confine::resolve(&root)];
        assert!(programs(&c, &temp).is_err());
        // A folder holding one it writes, or a temp folder: refused.
        let holding = running(&[root.join("out")], &[root.to_path_buf()]);
        let err = programs(&holding, &[]).unwrap_err().to_string();
        assert!(err.contains("holds"), "{err}");
        // One not there yet is judged as a folder.
        let later = running(&[root.join("later/out")], &[root.join("later")]);
        let err = programs(&later, &[]).unwrap_err().to_string();
        assert!(err.contains("holds"), "{err}");
        let temp = [link_sandbox_confine::resolve(&bin.join("cache"))];
        assert!(programs(&c, &temp).is_err());
        // In a folder any user writes, the machine's own: refused.
        let temp = link_sandbox_confine::programs::temp_folders();
        for shared in ["/tmp/tool", "/var/tmp/tool", "/dev/shm/tool"] {
            let there = running(&[], &[PathBuf::from(shared)]);
            assert!(programs(&there, &temp).is_err(), "{shared}");
        }
        // A toolchain's build folder, where it writes: run by design.
        let build = root.join("target");
        let mut building = running(&[root.to_path_buf()], std::slice::from_ref(&build));
        building.builds = vec![build.clone()];
        assert!(programs(&building, &[]).is_ok());
        // Any: none to judge.
        let mut any = c;
        any.commands = Programs::Any;
        assert_eq!(programs(&any, &[]).unwrap(), Programs::Any);
    }

    #[cfg(unix)]
    #[test]
    fn what_a_spec_opens_is_its_resolved_grants_not_its_text() {
        let root = link_test_kit::TempDir::new("spec-opens");
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let link = root.join("work");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        let sh = vec![PathBuf::from("/nowhere/a/sh")];
        let c = running(std::slice::from_ref(&link), &sh);
        let before = opens(&c, Some(&sh));
        // The same text, the link moved: something else opened.
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&b, &link).unwrap();
        assert_ne!(opens(&c, Some(&sh)), before);
        let other = [PathBuf::from("/nowhere/b/sh")];
        assert_ne!(opens(&c, Some(&other)), opens(&c, Some(&sh)));
        // The question shows where it really is, and the program found.
        let lines = describe(&c).join("\n");
        let real = link_sandbox_confine::resolve(&b);
        assert!(
            lines.contains(&format!("(really {})", real.display())),
            "{lines}"
        );
        assert!(lines.contains("runs /nowhere/a/sh"), "{lines}");
    }

    #[test]
    fn what_a_spec_opened_before_is_remembered_as_it_was() {
        // A spec run before this version: the same lines, so not asked
        // again.
        let c = confined(
            "[files]\nwrite = [\"/specs/w\"]\n[internet]\nhosts = [\"*\"]\n\
             [localhost]\nports = [8080]\nmodels = [11434]\n[browser]\nhosts = [\"*\"]",
        )
        .unwrap();
        assert_eq!(
            opens(&c, None),
            "write [\"/specs/w\"]\nread []\nhide []\nread_home false\ninternet Some([])\n\
             tcp []\nper_minute None\nlan []\nports {8080}\nmodels {11434}\ntools []\n\
             browser Some([])\ncommands any"
        );
        // Host: said loudly, and remembered as its own.
        let host = confined("[network]\nmode = \"host\"\n[localhost]\nports = [\"*\"]").unwrap();
        assert!(opens(&host, None).contains("network host, every port true"));
        assert!(describe(&host)[0].contains("with no proxy"));
    }

    #[test]
    fn every_private_address_is_refused_on_a_clouds_machine() {
        let sys = link_test_kit::TempDir::new("spec-sys");
        let dmi = sys.join("class/dmi/id");
        let machine = |files: &[(&str, &str)]| {
            let _ = std::fs::remove_dir_all(&sys);
            std::fs::create_dir_all(&dmi).unwrap();
            std::fs::create_dir_all(sys.join("hypervisor")).unwrap();
            for (file, text) in files {
                std::fs::write(sys.join(file), format!("{text}\n")).unwrap();
            }
            link_sandbox_proxy::lan::on_cloud_at(&sys)
        };
        for cloud in [
            &[("class/dmi/id/sys_vendor", "Amazon EC2")][..],
            // AWS's Xen machines (t2, m4): only the BIOS's version says so.
            &[
                ("class/dmi/id/sys_vendor", "Xen"),
                ("class/dmi/id/product_name", "HVM domU"),
                ("class/dmi/id/bios_version", "4.11.amazon"),
            ],
            &[("hypervisor/uuid", "ec2e1916-9099-7caf-fd21-012345abcdef")],
            &[("class/dmi/id/product_name", "Google Compute Engine")],
            &[("class/dmi/id/bios_vendor", "Google")],
            &[("class/dmi/id/chassis_asset_tag", "7783-7084-3265-9085")],
        ] {
            assert!(machine(cloud), "{cloud:?}");
        }
        for not in [
            &[][..],
            &[("class/dmi/id/sys_vendor", "LENOVO")],
            // A Surface laptop, Hyper-V here, a Chromebook.
            &[("class/dmi/id/sys_vendor", "Microsoft Corporation")],
            &[("class/dmi/id/sys_vendor", "Google")],
            &[
                ("class/dmi/id/sys_vendor", "Xen"),
                ("hypervisor/uuid", "1234-ec2"),
            ],
        ] {
            assert!(!machine(not), "{not:?}");
        }
        for whole in ["*", "0.0.0.0/0", "10.0.0.0/8", "::/0", "fc00::/7"] {
            let err = lan_here(&[whole.into()], true).unwrap_err().to_string();
            assert!(err.contains("list the addresses"), "{err}");
            lan_here(&[whole.into()], false).unwrap();
        }
        lan_here(&["10.0.0.5".into(), "10.1.0.0/16".into()], true).unwrap();
        // Ranges that hold a block between them are a block.
        let halves = ["10.0.0.0/9".to_string(), "10.128.0.0/9".into()];
        assert!(lan_here(&halves, true).is_err());
        lan_here(&halves[..1], true).unwrap();
        let four: Vec<String> = (0..4).map(|i| format!("192.168.{}.0/18", i * 64)).collect();
        assert!(lan_here(&four, true).is_err());
        lan_here(&four[1..], true).unwrap();
        // Host mode reaches all of it, and the metadata service.
        let host = confined("[network]\nmode = \"host\"").unwrap();
        let err = network_here(&host, true).unwrap_err().to_string();
        assert!(err.contains("isn't taken on a cloud's machine"), "{err}");
        network_here(&host, false).unwrap();
        let star = confined("[lan]\nhosts = [\"*\"]").unwrap();
        assert!(network_here(&star, true).is_err());
        network_here(&star, false).unwrap();
    }
}
