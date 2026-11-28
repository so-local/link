//! [`Rules`]: a [`Confinement`] resolved on this machine, every path
//! through its links, with what no confinement chooses (Link's settings
//! hidden, this program as the first inside), for a [`crate::Backend`].

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use link_sandbox_client::{Confinement, Net};
use link_secretive::Duplicate;

pub use crate::linux::Capabilities;

/// What a sandboxed program may touch: a [`Confinement`] resolved on this
/// machine, in the parts each [`crate::Backend`] enforces. A [`Net`] is
/// read once into the fields each backend uses, not flattened into one
/// shared set of booleans: Linux sees whether its own network is the
/// host's and which ports a relay carries in; macOS sees the localhost
/// rules its profile needs. The shared [`Common`] is what both walls
/// enforce alike.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rules {
    /// What every backend enforces alike.
    pub common: Common,
    /// What only the Linux backend enforces ([`crate::linux`]).
    pub linux: Linux,
    /// What only the macOS backend enforces ([`crate::macos`]).
    pub macos: Macos,
}

/// The file and program grants every backend enforces alike.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Common {
    /// Read and write.
    pub write: Vec<PathBuf>,
    /// Read only.
    pub read: Vec<PathBuf>,
    /// The user's home folder, for `read_home`.
    pub home: PathBuf,
    /// The whole home folder, read only.
    pub read_home: bool,
    /// Folders hidden whatever else is allowed: Link's settings.
    pub hide: Vec<PathBuf>,
    /// Installed programs are hidden unless explicitly read, and never writable.
    pub programs: Vec<PathBuf>,
    /// Folders the policy hides, whatever else is allowed but for what it
    /// names in them (`write`, `read`): other agents' folders.
    pub closed: Vec<PathBuf>,
    /// What runs outside the sandbox at the top of the folders it writes
    /// but for its policy's `write_dot_files` ([`crate::dotfiles`]): read
    /// only.
    pub outside: Vec<crate::dotfiles::Entry>,
    /// The only programs it may run (files, or folders of them), this
    /// program among them; None: any.
    pub commands: Option<Vec<PathBuf>>,
}

/// What only the Linux backend enforces (bubblewrap, Landlock, seccomp).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Linux {
    /// Keeps the caller's terminal as its controlling terminal.
    pub terminal: bool,
    /// Its own network is this machine's as it is ([`Net::Host`]): no
    /// `--unshare-net`, the internet and DNS reachable.
    pub host: bool,
    /// This machine's ports its network reaches, relayed in: its proxy's
    /// and models' ([`Net::Proxied`]), or the loopback's ([`Net::Loopback`]).
    pub ports: BTreeSet<u16>,
    /// Runtime folders the sandbox itself writes: its sockets' folder.
    /// Never a policy's, and never inside a Safe or installed programs.
    pub own: Vec<PathBuf>,
    /// `lnk-sandbox`, which starts every program in the sandbox under the
    /// seccomp filter (`sandbox confine`); None: no filter.
    pub confine: Option<PathBuf>,
    /// A file in place of `/etc/hosts`, naming the endpoints its proxy
    /// reaches.
    pub etc_hosts: Option<PathBuf>,
    /// A file in place of `/etc/resolv.conf`, naming its proxy's resolver
    /// in its own network (the proxy's DNS).
    pub resolv_conf: Option<PathBuf>,
    /// What this kernel and bubblewrap can enforce: which walls engage
    /// ([`Capabilities`]).
    pub caps: Capabilities,
}

/// What only the macOS backend enforces (a Seatbelt profile).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Macos {
    /// `/tmp/<prefix>*` and `/tmp/.<prefix>*`, read and write.
    pub tmp_prefix: Option<String>,
    /// This machine's network as it is ([`Net::Host`]): the internet and
    /// DNS, listening on every address.
    pub network: bool,
    /// Every localhost port ([`Net::Host`]'s `localhost`).
    pub localhost: bool,
    /// The localhost ports it may connect to without `localhost`.
    pub ports: BTreeSet<u16>,
    /// The localhost ports it may listen on without `network`: those it
    /// serves, never one it only reaches.
    pub serves: BTreeSet<u16>,
    /// Certificate checks without `network`: behind a proxy that reaches
    /// the internet, TLS still goes end to end, and the Security framework
    /// asks trustd.
    pub certificates: bool,
    /// Xcode's and the Command Line Tools' programs, read only and run.
    pub developer_tools: bool,
    /// With any program open: each one run is reported in the system log,
    /// for `lnk sandbox learn` ([`crate::macos::reports`]).
    pub report_execs: bool,
}

impl Rules {
    /// The rules for `confinement` here, each grant resolved to where it
    /// really is (Seatbelt matches real paths: `/private/tmp/...`, not
    /// `/tmp/...`). A folder that doesn't exist yet resolves through its
    /// nearest existing parent: the program creates it, with its own
    /// permissions.
    ///
    /// A grant reached through a symbolic link a non-root user controls
    /// is refused, naming the real path ([`resolve_grant`]): following it
    /// would let a sandboxed program, another agent with a writable
    /// shared folder, or another local user redirect the grant to what it
    /// never named. This is the one place a raw grant is resolved for a
    /// backend, so no wall follows a planted link.
    pub fn of(policy: &Confinement) -> anyhow::Result<Self> {
        Self::with_files(policy, false)
    }

    /// A policy whose file grants have already been resolved and checked
    /// ([`resolve_files`]). Never follow their names again after that
    /// authorization point, so no refusal can arise here.
    pub fn from_resolved(policy: &Confinement) -> Self {
        Self::with_files(policy, true).expect("already-resolved grants are not resolved again")
    }

    fn with_files(policy: &Confinement, resolved: bool) -> anyhow::Result<Self> {
        let real = |paths: &[PathBuf]| -> anyhow::Result<Vec<PathBuf>> {
            if resolved {
                Ok(paths.to_vec())
            } else {
                Ok(grants(paths)?)
            }
        };
        let write = real(&policy.write)?;
        let commands = match policy.commands.clone().listed() {
            None => None,
            Some(programs) => {
                // This program, which sets the sandbox up from inside and
                // is its $BROWSER and git's SSH.
                let me = std::env::current_exe().and_then(|e| e.canonicalize());
                let mut listed = Vec::new();
                for program in &programs {
                    listed.push(if resolved {
                        program.clone()
                    } else {
                        resolve_grant(program)?
                    });
                }
                listed.extend(me);
                Some(listed)
            }
        };
        let host = matches!(policy.net, Net::Host { .. });
        Ok(Self {
            common: Common {
                outside: outside(policy, &write, resolved),
                read: real(&policy.read)?
                    .into_iter()
                    .chain(ca_file(
                        std::env::var_os(crate::constants::CA_FILE).as_deref(),
                    ))
                    .collect(),
                write,
                home: resolve(&link_files::home_dir()),
                read_home: policy.read_home,
                hide: settings(),
                programs: vec![
                    resolve(
                        &link_files::try_installation_dir()?
                            .join(&link_files::layout::LAYOUT.boundaries.exec),
                    ),
                    resolve(&link_files::layout::SystemRoot::Exec.path()),
                ],
                // Never left out: a link only hides more.
                closed: if resolved {
                    policy.hide.clone()
                } else {
                    policy.hide.iter().map(|p| resolve(p)).collect()
                }
                .into_iter()
                .chain([
                    resolve(&link_files::layout::Root::Runtime.path()),
                    resolve(&link_files::layout::Hidden::AgentData.path()),
                    resolve(&link_files::layout::Hidden::SharedMemory.path()),
                    resolve(&link_files::layout::Hidden::SharedHarnesses.path()),
                    resolve(&link_files::layout::Hidden::TeamMemory.path()),
                ])
                .collect(),
                commands,
            },
            linux: Linux {
                terminal: policy.terminal,
                host,
                ports: ports(&policy.net),
                own: Vec::new(),
                confine: confiner(),
                etc_hosts: None,
                resolv_conf: None,
                caps: Capabilities::here(),
            },
            macos: Macos {
                tmp_prefix: policy.tmp_prefix.clone(),
                network: host,
                localhost: matches!(
                    policy.net,
                    Net::Host {
                        localhost: true,
                        ..
                    }
                ),
                ports: ports(&policy.net),
                serves: serves(&policy.net),
                certificates: false,
                developer_tools: policy.developer_tools,
                report_execs: false,
            },
        })
    }
}

/// The file of certificate authorities the program inside trusts, as the
/// environment it inherits names it (`SSL_CERT_FILE`), to read: that one
/// file, never its folder, and never to write, so a machine behind a
/// proxy that re-signs TLS (a company's) keeps its certificate wherever
/// it likes. None when `named` isn't an absolute path to a regular file,
/// or reaches one through a link another user owns ([`resolve_grant`]).
/// A folder of them (`SSL_CERT_DIR`) is never opened this way: a folder a
/// variable names may hold more than certificates, a key beside its
/// certificate or a whole home.
fn ca_file(named: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let named = Path::new(named?);
    if !named.is_absolute() {
        return None;
    }
    let real = resolve_grant(named).ok()?;
    std::fs::metadata(&real)
        .ok()
        .filter(|meta| meta.is_file())?;
    Some(real)
}

/// Freeze the filesystem part of a policy before checking or showing
/// its permissions, refusing a grant reached through a non-root link
/// ([`resolve_grant`]). Wrapping that result uses
/// [`Rules::from_resolved`], which never resolves the names again.
pub fn resolve_files(policy: &Confinement) -> anyhow::Result<Confinement> {
    let mut frozen = policy.duplicate();
    frozen.write = grants(&policy.write)?;
    frozen.read = grants(&policy.read)?;
    frozen.write_dot_files = grants(&policy.write_dot_files)?;
    frozen.hide = policy.hide.iter().map(|p| resolve(p)).collect();
    Ok(frozen)
}

fn grants(paths: &[PathBuf]) -> Result<Vec<PathBuf>, GrantRefused> {
    paths.iter().map(|p| resolve_grant(p)).collect()
}

/// This machine's ports a sandbox on `net` reaches: behind its proxy,
/// its models' too, which Linux relays in, filtered.
pub fn ports(net: &Net) -> BTreeSet<u16> {
    match net {
        Net::Closed => BTreeSet::new(),
        Net::Loopback { ports, .. } | Net::Reaching { ports, .. } | Net::Host { ports, .. } => {
            ports.clone()
        }
        Net::Proxied { proxy, ports, .. } => ports
            .iter()
            .chain(proxy.routes.iter().flat_map(|r| &r.ports))
            .copied()
            .collect(),
    }
}

/// The ports of this machine's loopback a sandbox on `net` serves: its
/// proxy's `serve`, only behind its proxy.
pub fn serves(net: &Net) -> BTreeSet<u16> {
    match net {
        Net::Proxied { proxy, .. } => proxy.serve.clone(),
        Net::Closed | Net::Loopback { .. } | Net::Reaching { .. } | Net::Host { .. } => {
            BTreeSet::new()
        }
    }
}

/// A grant refused because a symbolic link a non-root user controls lies
/// on the way to it ([`resolve_grant`]). The real path is named, since
/// the user can grant it directly.
#[derive(Debug)]
pub struct GrantRefused {
    /// The grant as it was named.
    pub grant: PathBuf,
    /// The component on the way that a non-root user controls.
    pub at: PathBuf,
    /// Where that link leads, when it reads as one.
    pub target: Option<PathBuf>,
}

impl std::fmt::Display for GrantRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not granting {}: ", self.grant.display())?;
        match &self.target {
            Some(target) => write!(
                f,
                "{} is a symbolic link to {}, which a non-root user controls; \
                 grant the real path instead",
                self.at.display(),
                target.display()
            ),
            None => write!(
                f,
                "{} can't be resolved without trusting what a non-root user controls; \
                 grant the real path instead",
                self.at.display()
            ),
        }
    }
}

impl std::error::Error for GrantRefused {}

/// `path` resolved to where it really is, following a symbolic link only
/// where root owns it (`/tmp` to `/private/tmp`, a distro's `/home`
/// link): the system's own layout, which no other user can have planted
/// or can swap. A link any other user owns is refused ([`GrantRefused`]),
/// at the grant's own root as much as below it, since a sandboxed
/// program, another agent with a writable shared folder, or another local
/// user could point it at `~/.ssh`, Link's settings, or anything else the
/// grant never named. A component that isn't there yet is taken literally
/// under the real parent: the program makes it, so no link can lie on the
/// way. Every raw grant reaching a backend passes here, and so does one
/// added to a resolved snapshot (a run's given folders, a session's).
pub fn resolve_grant(path: &Path) -> Result<PathBuf, GrantRefused> {
    use std::os::unix::fs::MetadataExt;
    use std::path::Component;
    let refuse = |at: PathBuf, target: Option<PathBuf>| GrantRefused {
        grant: path.to_path_buf(),
        at,
        target,
    };
    let mut real = PathBuf::from("/");
    // Once a component isn't there, nothing below it is either: the rest
    // is the program's to make, under the real parent, with no link to
    // follow.
    let mut missing = false;
    for part in path.components() {
        let part = match part {
            Component::Normal(part) => part,
            Component::ParentDir => {
                real.pop();
                continue;
            }
            Component::RootDir | Component::Prefix(_) | Component::CurDir => continue,
        };
        let next = real.join(part);
        if missing {
            real = next;
            continue;
        }
        match std::fs::symlink_metadata(&next) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if meta.uid() != 0 {
                    return Err(refuse(next.clone(), std::fs::read_link(&next).ok()));
                }
                // Root owns it, so no other user planted it or can swap
                // it, and only root chose where it leads: the system's own
                // link, followed whole.
                real = next
                    .canonicalize()
                    .map_err(|_| refuse(next.clone(), std::fs::read_link(&next).ok()))?;
            }
            Ok(_) => real = next,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                real = next;
                missing = true;
            }
            Err(_) => return Err(refuse(next, None)),
        }
    }
    Ok(real)
}

/// What runs outside the sandbox at the top of each folder `policy`
/// writes, but those it names in `write_dot_files`: of `write`, those
/// folders resolved.
fn outside(policy: &Confinement, write: &[PathBuf], resolved: bool) -> Vec<crate::dotfiles::Entry> {
    let open: Vec<PathBuf> = policy
        .write_dot_files
        .iter()
        .map(|p| if resolved { p.clone() } else { resolve(p) })
        .collect();
    let home = resolve(&link_files::home_dir());
    let mut folders: Vec<PathBuf> = Vec::new();
    for folder in write {
        if !open.contains(folder) && !folders.contains(folder) {
            folders.push(folder.clone());
        }
    }
    // Link's data folders, resolved as the folders are.
    let swept: Vec<PathBuf> = link_sandbox_contract::dot_files::swept_folders()
        .iter()
        .map(|p| resolve(p))
        .collect();
    folders
        .iter()
        .flat_map(|folder| {
            let data = link_sandbox_contract::dot_files::in_swept(folder, &swept);
            crate::dotfiles::of(folder, &home, data)
        })
        .collect()
}

/// `path` with its nearest existing ancestor resolved through symlinks.
pub fn resolve(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut existing = path;
    loop {
        if let Ok(real) = existing.canonicalize() {
            return rest.iter().rev().fold(real, |p, part| p.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Personal and system control stay closed to every run. Personal Safes
/// exist before launch so a later child cannot create either first.
fn settings() -> Vec<PathBuf> {
    let machine = link_files::installation_dir();
    let mut safes = vec![
        resolve(&machine.join(&link_files::layout::LAYOUT.boundaries.hidden_safe)),
        resolve(&link_files::layout::Home::Safe.path()),
    ];
    safes.extend(
        link_files::layout::system_safes()
            .iter()
            .map(|path| resolve(path)),
    );
    safes
}

/// This program, as a sandbox's first program on Linux (`sandbox
/// confine`): it closes every descriptor it was handed past stderr, and
/// where it's [`crate::linux::FILTERED`], installs the seccomp filter, then becomes the
/// program.
fn confiner() -> Option<PathBuf> {
    cfg!(target_os = "linux")
        .then(|| {
            std::env::current_exe()
                .and_then(|exe| exe.canonicalize())
                .ok()
        })
        .flatten()
}

/// `path` (absolute) without `.` and with each `..` taking off the part
/// before it, as a shell's `cd` does, without reading the disk.
pub fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A symbolic link `at` a non-root user owns: made as this user, and
    /// where that user is root, given to `nobody` so it reads as another
    /// user's. None where neither can make such a link (a non-root user
    /// can't give a link away). Its own ownership (`lchown`), not its
    /// target's.
    fn non_root_link(at: &Path, target: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;
        std::os::unix::fs::symlink(target, at).unwrap();
        if std::fs::symlink_metadata(at).unwrap().uid() == 0 {
            // Running as root: hand the link itself to another user.
            if std::os::unix::fs::lchown(at, Some(65534), None).is_err() {
                return false;
            }
        }
        std::fs::symlink_metadata(at).unwrap().uid() != 0
    }

    /// The CA file the environment names is read where it's a regular
    /// file at an absolute path, and never a folder, a missing file, a
    /// relative name or a file through another user's link.
    #[test]
    fn only_a_named_ca_file_is_read() {
        let temp = link_test_kit::TempDir::new("ca-file");
        let root = temp.canonicalize().unwrap();
        let file = root.join("corporate.pem");
        std::fs::write(&file, "-----BEGIN CERTIFICATE-----\n").unwrap();
        assert_eq!(ca_file(Some(file.as_os_str())), Some(file.clone()));
        assert_eq!(ca_file(Some(root.as_os_str())), None);
        assert_eq!(ca_file(Some(root.join("missing.pem").as_os_str())), None);
        assert_eq!(ca_file(Some("corporate.pem".as_ref())), None);
        assert_eq!(ca_file(None), None);
        let link = root.join("link.pem");
        if non_root_link(&link, &file) {
            assert_eq!(ca_file(Some(link.as_os_str())), None);
        }
    }

    #[test]
    fn a_named_program_resolves_installed_links_before_grant_validation() {
        use link_sandbox_client::{At, Programs, Way};

        let temp = link_test_kit::TempDir::new("homebrew-command");
        let root = temp.canonicalize().unwrap();
        let bin = root.join("bin");
        let package = root.join("Cellar/python");
        let executable = package.join("Frameworks/Python.framework/bin/python3.14");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(package.join("bin")).unwrap();
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        link_test_kit::write_program(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        let alias = bin.join("python3");
        if !non_root_link(&alias, Path::new("../Cellar/python/bin/python3")) {
            link_test_kit::gate::lacks(
                "a link another user owns",
                "root here can't hand a link to nobody (lchown is refused)",
                None,
            );
            return;
        }
        assert!(non_root_link(
            &package.join("bin/python3"),
            Path::new("../Frameworks/Python.framework/bin/python3.14"),
        ));
        let mut spec: link_sandbox_contract::spec::Spec =
            link_spec::parse("[commands]\nallow = [\"python3\"]").unwrap();
        let confine = |spec: &_| {
            Confinement::of(
                spec,
                At {
                    name: "homebrew",
                    dir: &root,
                    home: &root,
                    path: Some(bin.as_os_str()),
                    way: Way::Reaching(Vec::new()),
                },
            )
            .unwrap()
        };
        let confinement = confine(&spec);
        assert_eq!(
            confinement.commands,
            Programs::Only(vec![executable.clone()])
        );

        // Changing Homebrew's alias after discovery cannot redirect the
        // executable grant handed to the sandbox at startup.
        let other = root.join("other");
        link_test_kit::write_program(&other, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::remove_file(&alias).unwrap();
        assert!(non_root_link(&alias, &other));
        let rules = Rules::of(&confinement).unwrap();
        let commands = rules.common.commands.unwrap();
        assert!(commands.contains(&executable));
        assert!(!commands.contains(&other));

        // An explicit path is still a raw grant: discovery must not
        // weaken the refusal of user-controlled links at the wall.
        spec.commands.as_mut().unwrap().allow = Some(vec![alias.display().to_string()]);
        let explicit = confine(&spec);
        assert_eq!(explicit.commands, Programs::Only(vec![alias]));
        assert!(Rules::of(&explicit).is_err());
    }

    #[test]
    fn a_grant_through_a_non_root_link_is_refused_and_names_the_real_path() {
        let temp = link_test_kit::TempDir::new("grant-non-root-link");
        let root = temp.canonicalize().unwrap();
        let secret = root.join("secret");
        std::fs::create_dir_all(secret.join("key")).unwrap();

        // A grant that is itself a non-root link: refused, the target
        // named, rather than followed to the secret it never named.
        let grant = root.join("shared");
        if !non_root_link(&grant, &secret) {
            link_test_kit::gate::lacks(
                "a link another user owns",
                "root here can't hand a link to nobody (`lchown` is refused)",
                None,
            );
            return;
        }
        let refused = resolve_grant(&grant.join("key")).unwrap_err();
        assert_eq!(refused.grant, grant.join("key"));
        assert_eq!(refused.at, grant);
        assert_eq!(refused.target.as_deref(), Some(secret.as_path()));

        // A non-root link below the grant's own root, where a program
        // swapped a folder for one: refused too.
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let planted = project.join("sub");
        assert!(non_root_link(&planted, &secret));
        assert!(resolve_grant(&project.join("sub/key")).is_err());
    }

    #[test]
    fn a_grant_resolves_where_no_non_root_link_lies_on_the_way() {
        let temp = link_test_kit::TempDir::new("grant-plain");
        let root = temp.canonicalize().unwrap();
        let app = root.join("code/app");
        std::fs::create_dir_all(&app).unwrap();
        // A folder that's there, and one it will make under it.
        assert_eq!(resolve_grant(&app).unwrap(), app);
        assert_eq!(
            resolve_grant(&app.join("build/out")).unwrap(),
            app.join("build/out")
        );
    }
}
