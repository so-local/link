//! The system files exposed by the confinement backends.
//!
//! The Linux system baseline — the read-only system folders ([`SYSTEM`]),
//! the machine's `/etc` ([`LINUX_ETC`]), and the fresh writable folders
//! ([`LINUX_EPHEMERAL`]) — is declared once here and rendered by both
//! Linux walls: bubblewrap's mounts ([`crate::linux::bwrap::args`]) and
//! Landlock's second, independent file wall ([`crate::linux::enter`]).
//! The two must name the same files or they drift — one too tight to run
//! real programs, the other looser than the mounts it is meant to double.
//! `the_file_wall_is_a_subset_of_the_mounts` ([`crate::tests`]) holds them
//! to this one declaration.

pub use link_sandbox_client::LINUX_READS as SYSTEM;

/// Folders of other programs' sockets inside [`SYSTEM`]'s, each covered
/// by an empty one where it's there, since a socket's mode, not a
/// read-only mount, decides who connects. Multi-user Nix's daemon
/// (`/nix/var/nix/daemon-socket`) builds whatever it's asked outside the
/// sandbox, with the network for a build whose output is fixed.
pub const LINUX_SYSTEM_SOCKETS: &[&str] = &["/nix/var/nix/daemon-socket"];

/// The machine's `/etc`, shown read only. bubblewrap gives a program
/// [`ETC`] of it at `/etc` (a tmpfs made read-only once every mount in it
/// is made), and Landlock's file wall allows reading `/etc`; both name it
/// here, so neither can forget it while the other keeps it.
pub const LINUX_ETC: &str = "/etc";

/// A folder the Linux sandbox gives a program fresh, empty and writable,
/// and the bubblewrap flag that makes it. Both walls name every
/// [`LINUX_EPHEMERAL`] entry: bubblewrap makes each, and Landlock's file
/// wall allows reading and writing each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ephemeral {
    /// Where it is, e.g. `/tmp`.
    pub at: &'static str,
    /// The bubblewrap flag that makes it: `--tmpfs`, `--dev` or `--proc`.
    pub flag: &'static str,
}

impl Ephemeral {
    /// A fresh in-memory filesystem of its own.
    const fn tmpfs(at: &'static str) -> Self {
        Self {
            at,
            flag: "--tmpfs",
        }
    }
    /// A minimal `/dev` of its own.
    const fn dev(at: &'static str) -> Self {
        Self { at, flag: "--dev" }
    }
    /// A `/proc` for its own process namespace.
    const fn proc(at: &'static str) -> Self {
        Self { at, flag: "--proc" }
    }
}

/// The folders the Linux sandbox gives a program fresh, empty and
/// writable, so it holds nothing of another program's and leaves no file
/// for one. The order is the order bubblewrap's arguments take them;
/// Landlock's allow-set is a set, so the order does not reach it.
pub const LINUX_EPHEMERAL: &[Ephemeral] = &[
    Ephemeral::dev("/dev"),
    Ephemeral::proc("/proc"),
    // Every folder any user writes is its own, empty: nothing of another
    // program's, nor a way to leave a file for one.
    Ephemeral::tmpfs("/tmp"),
    Ephemeral::tmpfs("/var/tmp"),
    Ephemeral::tmpfs("/dev/shm"),
    // No sockets of the user's session (the systemd user bus could start
    // anything outside the sandbox).
    Ephemeral::tmpfs("/run"),
];

/// What Linux's sandbox shows a program of `/etc`, read only, those that
/// exist: what programs read to load, find users and names, check
/// certificates, tell the time and the system, and start a shell. A
/// name ending `*` is every entry it begins. Nothing else of `/etc` is
/// there: not its secrets (`shadow`, SSH's host keys, `ssl/private`,
/// a relay's settings, WireGuard's keys), which a sandbox run by root could
/// read whatever their modes, nor what's put there by hand
/// (`environment`, `profile.d`, `pip.conf`, `npmrc`, `fstab`), nor
/// `machine-id`, nor what can hold a token (`gitconfig`'s headers,
/// `nix`'s access tokens, `wgetrc`).
pub const ETC: &[&str] = &[
    "alternatives",
    "bash.bashrc",
    "bashrc",
    "ca-certificates",
    "ca-certificates.conf",
    "crypto-policies",
    "debian_version",
    "fonts",
    "gai.conf",
    "gnutls",
    "group",
    "host.conf",
    "hosts",
    "inputrc",
    "java-*",
    "ld.so.cache",
    "ld.so.conf",
    "ld.so.conf.d",
    "locale.alias",
    "locale.conf",
    "localtime",
    "lsb-release",
    "magic",
    "magic.mime",
    "mime.types",
    "nanorc",
    "networks",
    "nsswitch.conf",
    "os-release",
    "passwd",
    "perl",
    "pki",
    "profile",
    "profiles",
    "protocols",
    "python*",
    "redhat-release",
    "resolv.conf",
    "rpc",
    "services",
    "shells",
    "ssh/ssh_config",
    "ssh/ssh_config.d",
    "ssh/ssh_known_hosts",
    "ssl",
    "static",
    "system-release",
    "terminfo",
    "timezone",
    "vim",
    "vimrc",
    "xdg",
    "xml",
];

/// The names never shown in [`ETC`], at any depth, a link so named
/// too: private keys kept beside the certificates (`ssl/private`,
/// `pki/tls/private`, ...).
pub const ETC_HIDDEN: &[&str] = &["private"];
