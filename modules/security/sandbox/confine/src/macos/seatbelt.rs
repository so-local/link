//! The Seatbelt profile for a sandbox's [`Rules`], run by
//! `/usr/bin/sandbox-exec`: deny by default, the mechanism Claude Code
//! and Codex use.

use std::path::{Path, PathBuf};

use crate::{Common, Macos};

/// A Seatbelt profile reads the file and program grants every backend
/// shares (`common`) and the parts only macOS enforces (`macos`): never
/// anything Linux's bubblewrap would.
struct Rules<'a> {
    common: &'a Common,
    macos: &'a Macos,
}

/// The Seatbelt profile for `common` and `macos` (paths already resolved).
pub fn profile(common: &Common, macos: &Macos) -> String {
    let policy = &Rules { common, macos };
    // A file as itself (this program), anything else as a folder.
    let subpaths = |paths: &[PathBuf]| -> String {
        paths
            .iter()
            .map(|p| {
                let kind = if p.is_file() { "literal" } else { "subpath" };
                format!("\n  ({kind} {})", literal(&folder(p)))
            })
            .collect()
    };
    let write = subpaths(&policy.common.write);
    let tmp = tmp_filter(policy.macos.tmp_prefix.as_deref(), "regex").unwrap_or_default();
    // Unix sockets: only in its own folders, one rule per folder.
    let filters: Vec<String> = policy
        .common
        .write
        .iter()
        .map(|p| format!("(subpath {})", literal(&p.display().to_string())))
        .chain(
            tmp_filter(policy.macos.tmp_prefix.as_deref(), "path-regex")
                .map(|t| t.trim().to_string()),
        )
        .collect();
    let socket_paths: String = filters
        .iter()
        .map(|f| {
            format!(
                "\n(allow network-bind (local unix-socket {f}))\n(allow network-outbound (remote unix-socket {f}))"
            )
        })
        .collect();

    let mut profile = format!(
        r#"(version 1)
; Written by lnk. A program reads the system's programs and libraries, reads
; and writes only the folders below, and talks to localhost. The permissions
; it was allowed follow.
(deny default)
{etc}
{exec}
(allow signal process-info* mach-priv-task-port (target same-sandbox))
(allow pseudo-tty)
; Hardware and OS facts only: a broad sysctl-read would also give other
; processes' arguments and environments (kern.procargs2). The list follows
; Anthropic's sandbox-runtime (Apache-2.0).
(allow sysctl-read
  (sysctl-name "hw.activecpu")
  (sysctl-name "hw.busfrequency_compat")
  (sysctl-name "hw.byteorder")
  (sysctl-name "hw.cacheconfig")
  (sysctl-name "hw.cachelinesize_compat")
  (sysctl-name "hw.cpufamily")
  (sysctl-name "hw.cpufrequency")
  (sysctl-name "hw.cpufrequency_compat")
  (sysctl-name "hw.cputype")
  (sysctl-name "hw.l1dcachesize_compat")
  (sysctl-name "hw.l1icachesize_compat")
  (sysctl-name "hw.l2cachesize_compat")
  (sysctl-name "hw.l3cachesize_compat")
  (sysctl-name "hw.logicalcpu")
  (sysctl-name "hw.logicalcpu_max")
  (sysctl-name "hw.machine")
  (sysctl-name "hw.memsize")
  (sysctl-name "hw.ncpu")
  (sysctl-name "hw.nperflevels")
  (sysctl-name "hw.packages")
  (sysctl-name "hw.pagesize_compat")
  (sysctl-name "hw.pagesize")
  (sysctl-name "hw.physicalcpu")
  (sysctl-name "hw.physicalcpu_max")
  (sysctl-name "hw.tbfrequency_compat")
  (sysctl-name "hw.vectorunit")
  (sysctl-name "kern.argmax")
  (sysctl-name "kern.bootargs")
  (sysctl-name "kern.boottime")
  (sysctl-name "kern.bootsessionuuid")
  (sysctl-name "kern.hostname")
  (sysctl-name "kern.maxfiles")
  (sysctl-name "kern.maxfilesperproc")
  (sysctl-name "kern.maxproc")
  (sysctl-name "kern.ngroups")
  (sysctl-name "kern.osproductversion")
  (sysctl-name "kern.osrelease")
  (sysctl-name "kern.ostype")
  (sysctl-name "kern.osvariant_status")
  (sysctl-name "kern.osversion")
  (sysctl-name "kern.secure_kernel")
  (sysctl-name "kern.tcsm_available")
  (sysctl-name "kern.tcsm_enable")
  (sysctl-name "kern.usrstack64")
  (sysctl-name "kern.version")
  (sysctl-name "kern.willshutdown")
  (sysctl-name "machdep.cpu.brand_string")
  (sysctl-name "machdep.ptrauth_enabled")
  (sysctl-name "security.mac.lockdown_mode_state")
  (sysctl-name "sysctl.proc_cputype")
  (sysctl-name "vm.loadavg")
  (sysctl-name-prefix "hw.optional.arm")
  (sysctl-name-prefix "hw.optional.arm.")
  (sysctl-name-prefix "hw.optional.armv8_")
  (sysctl-name-prefix "hw.perflevel")
  (sysctl-name-prefix "kern.proc.all")
  (sysctl-name-prefix "kern.proc.pgrp.")
  (sysctl-name-prefix "kern.proc.pid.")
  (sysctl-name-prefix "machdep.cpu.")
  (sysctl-name-prefix "net.routetable.")
  (sysctl-name "kern.tcsm_enable"))
; V8 (Node) sets this.
(allow sysctl-write (sysctl-name "kern.tcsm_enable"))
; Looking up users and groups, the temp folder, logging.
(allow mach-lookup
  (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.system.opendirectoryd.membership")
  (global-name "com.apple.bsd.dirhelper")
  (global-name "com.apple.system.logger")
  (global-name "com.apple.logd")
  (global-name "com.apple.system.notification_center"))
; Serving: see the section after these (listening on localhost only,
; unless network). Connecting: see the localhost section below.
(allow system-socket (socket-domain AF_UNIX))
(allow system-socket (require-all (socket-domain AF_SYSTEM) (socket-protocol 2)))
; Unix sockets only in its own folders.{sockets}
; stat() on directories (realpath walks them), never on files: it can't
; tell which files exist outside its folders.
(allow file-read-metadata (vnode-type DIRECTORY))
; The root's symlinks into /private, which every /tmp, /var and /etc path
; goes through.
(allow file-read-metadata (literal "/tmp") (literal "/var") (literal "/etc"))
; dyld aborts before exec if it can't read the root itself.
(allow file-read* (literal "/"))
(allow file-ioctl
  (literal "/dev/null")
  (literal "/dev/zero")
  (literal "/dev/random")
  (literal "/dev/urandom")
  (literal "/dev/tty")
  (literal "/dev/dtracehelper"))
(allow file-read* file-map-executable
  (subpath "/usr/bin")
  (subpath "/usr/lib")
  (subpath "/usr/libexec")
  (subpath "/usr/sbin")
  (subpath "/usr/share")
  (subpath "/usr/standalone")
  (subpath "/bin")
  (subpath "/sbin")
  (subpath "/System")
  (subpath "/Library/Apple")
  ; Homebrew's programs and libraries (/opt/homebrew, and /usr/local on
  ; Intel), not its var/ (local databases, logs) nor most of its etc/
  ; (servers' settings, with their passwords): only the certificates.{homebrew}
  (subpath "/private/var/db/timezone")
  ; macOS's choices of /bin/sh and developer tools (links only).
  (subpath "/private/var/select")
  ; Devices: only these, not the user's other terminals.
  (literal "/dev/null")
  (literal "/dev/zero")
  (literal "/dev/random")
  (literal "/dev/urandom")
  (literal "/dev/tty")
  (literal "/dev/dtracehelper")
  (literal "/dev/autofs_nowait")
  (literal "/dev/stdin")
  (literal "/dev/stdout")
  (literal "/dev/stderr")
  (regex #"^/dev/fd/")
  ; The user's locale and region, which Node and CoreFoundation read.
  (regex #"/Library/Preferences/\.GlobalPreferences[^/]*\.plist$"){read}{write}{tmp})
(allow file-write*
  (literal "/dev/null")
  (literal "/dev/dtracehelper")
  (literal "/dev/tty")
  (regex #"^/dev/fd/"){write}{tmp})
{listen}"#,
        read = subpaths(&policy.common.read),
        exec = exec_rules(policy),
        etc = etc_rules(),
        sockets = socket_paths,
        homebrew = homebrew(),
        listen = listen_rules(policy),
    );
    if policy.macos.network {
        profile += r#"; network: the internet and DNS.
(allow network-outbound (remote ip "*:*"))
(allow network-outbound (remote unix-socket (path-literal "/private/var/run/mDNSResponder")))
(allow mach-lookup
  (global-name "com.apple.SystemConfiguration.configd")
  (global-name "com.apple.SystemConfiguration.DNSConfiguration")
  (global-name "com.apple.dnssd.service")
  (global-name "com.apple.networkd")
  ; Certificate checks (TLS through macOS's Security framework, as
  ; Python's truststore does): trustd answers from the system's roots.
  (global-name "com.apple.trustd")
  (global-name "com.apple.trustd.agent")
  (global-name "com.apple.ocspd"))
"#;
    } else if policy.macos.certificates {
        profile += r#"; Behind its proxy: certificate checks only (TLS through macOS's
; Security framework, end to end through the proxy), no DNS or network
; settings, which the proxy does outside.
(allow mach-lookup
  (global-name "com.apple.trustd")
  (global-name "com.apple.trustd.agent"))
"#;
    }
    // Last: a later rule wins, so this narrows the network grant's "*:*".
    profile += &localhost_rules(policy);
    if policy.macos.developer_tools {
        profile += &format!(
            "; developer-tools: /usr/bin/git and friends run the real ones through\n\
             ; xcrun, from the Command Line Tools or Xcode: read, never written, and\n\
             ; run where [commands] names programs (see their rule).\n\
             (allow file-read* file-map-executable{DEVELOPER_TOOLS}\n  \
             (literal \"/private/var/db/xcode_select_link\"))\n"
        );
        profile += r#"; xcrun refuses to run tools ("You have not agreed to the Xcode license")
; unless it can read that the license was accepted: Xcode's preferences,
; that one domain only (cfprefsd checks the sandbox for each domain).
(allow file-read* (literal "/Library/Preferences/com.apple.dt.Xcode.plist"))
(allow user-preference-read (preference-domain "com.apple.dt.Xcode"))
(allow mach-lookup
  (global-name "com.apple.cfprefsd.daemon")
  (global-name "com.apple.cfprefsd.agent"))
"#;
    }
    if policy.common.read_home {
        profile += &format!(
            "; read-home: the whole home folder, read only.\n(allow file-read* (subpath {}))\n",
            literal(&policy.common.home.display().to_string())
        );
    }
    // After read-home, so it can't open them: what the policy hides, then
    // what it names in there, which stays open.
    for closed in &policy.common.closed {
        profile += &format!(
            "; Hidden, whatever else is allowed.\n(deny file-read* file-write* (subpath {}))\n",
            literal(&closed.display().to_string())
        );
    }
    let inside = |p: &PathBuf| {
        policy
            .common
            .closed
            .iter()
            .any(|c| p.starts_with(c) && p != c)
    };
    for path in policy.common.read.iter().filter(|p| inside(p)) {
        profile += &format!(
            "(allow file-read* (subpath {}))\n",
            literal(&path.display().to_string())
        );
    }
    for path in policy.common.write.iter().filter(|p| inside(p)) {
        let path = literal(&path.display().to_string());
        profile +=
            &format!("(allow file-read* (subpath {path}))\n(allow file-write* (subpath {path}))\n");
    }
    profile += &outside_rules(policy);
    // Last, so nothing opens them: Link's own settings (tokens, keys,
    // tools, these rules), to reading and writing.
    for hidden in &policy.common.hide {
        profile += &format!(
            "; Link's own settings, whatever else is allowed.\n(deny file-read* file-write* (subpath {}))\n",
            literal(&hidden.display().to_string())
        );
    }
    for programs in &policy.common.programs {
        profile += &format!(
            "(deny file-read* file-write* (subpath {}))\n",
            literal(&programs.display().to_string())
        );
    }
    // Only the installed program trees explicitly named for reading.
    for program in policy.common.read.iter().filter(|p| {
        policy
            .common
            .programs
            .iter()
            .any(|root| p.starts_with(root) && *p != root)
    }) {
        profile += &format!(
            "(allow file-read* (subpath {}))\n",
            literal(&program.display().to_string())
        );
    }
    profile
}

/// The same public system settings as Linux, never the whole of /etc.
/// Private certificate keys stay closed inside otherwise readable
/// certificate folders. Explicit file grants follow these system rules.
fn etc_rules() -> String {
    let mut rules = String::from("(allow file-read* file-map-executable");
    for name in crate::system::ETC {
        match name.strip_suffix('*') {
            Some(prefix) => {
                let prefix = prefix.replace('.', "\\.");
                rules += &format!("\n  (regex #\"^/private/etc/{prefix}[^/]*(/|$)\")");
            }
            None => rules += &format!("\n  (subpath \"/private/etc/{name}\")"),
        }
    }
    rules += ")\n(deny file-read* file-map-executable";
    for name in crate::system::ETC_HIDDEN {
        rules += &format!("\n  (regex #\"^/private/etc/(.*/)?{name}(/|$)\")");
    }
    rules += ")\n";
    rules
}

/// What runs outside the sandbox at the top of each folder it writes,
/// read only, by path whether it's there or not, and whatever the case
/// of its letters; what a link there points at too. Then any folder it
/// writes inside one, open again: a later rule wins.
fn outside_rules(policy: &Rules) -> String {
    let mut denied = String::new();
    let mut again = String::new();
    for entry in &policy.common.outside {
        let path = entry.path();
        if policy.common.write.contains(&path) {
            continue;
        }
        // A pinned folder itself only (renamed, removed), as a file is.
        let file = entry.pin || (entry.kind == crate::dotfiles::Kind::File && entry.exists);
        denied += &format!(
            "\n  (regex #\"{}\")",
            crate::dotfiles::seatbelt_regex(&entry.folder, &entry.rel, file)
        );
        if entry.kind == crate::dotfiles::Kind::Link
            && let Ok(target) = path.canonicalize()
        {
            denied += &format!("\n  (subpath {})", literal(&target.display().to_string()));
        }
        for write in policy
            .common
            .write
            .iter()
            .filter(|w| w.starts_with(&path) && **w != path)
        {
            again += &format!(
                "(allow file-write* (subpath {}))\n",
                literal(&write.display().to_string())
            );
        }
    }
    if denied.is_empty() {
        return String::new();
    }
    format!(
        "; What runs outside the sandbox from the top of a folder it writes (git's\n\
         ; settings and hooks, .envrc, an editor's tasks): read only.\n\
         (deny file-write*{denied})\n{again}"
    )
}

/// Which programs it may run: any, or only its `commands` (a file, or
/// every file under a folder).
fn exec_rules(policy: &Rules) -> String {
    let Some(programs) = &policy.common.commands else {
        return match policy.macos.report_execs {
            // `lnk sandbox learn`: each one in the system log.
            true => "(allow process-fork)\n(allow process-exec (with report))".into(),
            false => "(allow process-exec process-fork)".into(),
        };
    };
    let mut rules =
        String::from("(allow process-fork)\n; Only the programs it may run.\n(allow process-exec");
    for program in programs {
        let path = literal(&folder(program));
        match program.is_dir() {
            true => rules += &format!("\n  (subpath {path})"),
            false => rules += &format!("\n  (literal {path})"),
        }
    }
    // macOS's /bin/sh runs the shell chosen in /private/var/select/sh
    // (bash, unless changed): with /bin/sh, that one too.
    if programs.iter().any(|p| p.as_path() == Path::new(SH))
        && let Some(variant) = variant(Path::new(SH_SELECTED))
    {
        rules += &format!("\n  (literal {})", literal(&variant.display().to_string()));
    }
    // The developer tools' own programs, which /usr/bin/git and friends
    // run (xcrun, and on a Mac with Xcode alone, its xcodebuild).
    if policy.macos.developer_tools {
        rules += DEVELOPER_TOOLS;
    }
    rules.push(')');
    rules
}

/// macOS's shell trampoline, which runs the shell [`SH_SELECTED`] names.
const SH: &str = "/bin/sh";

/// The link naming the shell macOS's `/bin/sh` runs.
const SH_SELECTED: &str = "/private/var/select/sh";

/// The developer tools' folders, which `developer_tools` opens to read
/// and to run: the Command Line Tools and every Xcode.
const DEVELOPER_TOOLS: &str =
    "\n  (subpath \"/Library/Developer\")\n  (regex #\"^/Applications/Xcode[^/]*\\.app/\")";

/// `path` as Seatbelt matches it: a folder written with a final `/`
/// (a toolchain's `~/.local/share/uv/python/`) without it, since a
/// `subpath` ending in `/` matches nothing below it.
fn folder(path: &Path) -> String {
    let text = path.display().to_string();
    match text.trim_end_matches('/') {
        "" => text,
        trimmed => trimmed.to_string(),
    }
}

/// The program the link `selected` leads to, resolved; none without one.
fn variant(selected: &Path) -> Option<PathBuf> {
    std::fs::read_link(selected).ok()?;
    selected.canonicalize().ok()
}

/// Homebrew's folders a program may read, under each of its prefixes.
const HOMEBREW: &[&str] = &[
    "bin",
    "sbin",
    "lib",
    "libexec",
    "Cellar",
    "opt",
    "share",
    "Frameworks",
    "include",
    "Homebrew",
];

fn homebrew() -> String {
    let mut rules = String::new();
    for prefix in ["/opt/homebrew", "/usr/local"] {
        for dir in HOMEBREW {
            rules += &format!("\n  (subpath \"{prefix}/{dir}\")");
        }
        rules += &format!("\n  (regex #\"^{prefix}/etc/(openssl[^/]*|ca-certificates)(/|$)\")");
    }
    rules
}

/// Where it may listen. Without `network`, on localhost only, and only on
/// its own ports: a connection it accepts from the network goes both
/// ways, so it would be a way out, and a port of another agent's, taken
/// while that agent is stopped, would be taken for part of it. ("localhost"
/// doesn't match a dual-stack socket's ::ffff:127.0.0.1: a program without
/// `network` binds 127.0.0.1 or ::1.)
fn listen_rules(policy: &Rules) -> String {
    if policy.macos.network {
        return "; network: listening on every address.\n\
                (allow network-bind network-inbound (local ip \"*:*\"))\n"
            .into();
    }
    let mut rules = String::from(
        "; Listening: on localhost only, on its own ports (network opens every\n\
         ; address).\n",
    );
    for port in &policy.macos.ports {
        rules += &format!("(allow network-bind network-inbound (local ip \"localhost:{port}\"))\n");
    }
    rules
}

/// Which localhost ports it may connect to: all with `localhost`, else
/// only `policy.macos.ports`. With `network`, "*:*" also matched localhost, so
/// localhost is denied again first.
fn localhost_rules(policy: &Rules) -> String {
    if policy.macos.localhost {
        return "; localhost: every port on this machine.\n\
                (allow network-outbound (remote ip \"localhost:*\"))\n"
            .into();
    }
    let mut rules = String::from(
        "; Localhost: only the ports its policy names (the localhost permission\n\
         ; opens the rest).\n",
    );
    if policy.macos.network {
        // A gap: "localhost" doesn't match ::ffff:127.0.0.1 (IPv4 localhost
        // through an IPv6 socket), so "*:*" still lets that through, and a
        // filter names no other host. `(remote ip6 "*:*")` matched IPv4
        // too (it blocked the internet), so it can't close it.
        rules += "(deny network-outbound (remote ip \"localhost:*\"))\n";
    }
    for port in &policy.macos.ports {
        rules += &format!("(allow network-outbound (remote ip \"localhost:{port}\"))\n");
    }
    rules
}

/// The filter for `/tmp/<prefix>*` (`/private/tmp` is its real path):
/// `regex` for files, `path-regex` for unix sockets.
pub(crate) fn tmp_filter(prefix: Option<&str>, kind: &str) -> Option<String> {
    // Harness names are letters, digits and dashes: nothing to escape.
    let prefix = prefix
        .filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))?;
    Some(format!("\n  ({kind} #\"^/private/tmp/\\.?{prefix}\")"))
}

/// An SBPL string literal.
pub(crate) fn literal(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod etc_tests {
    use super::*;

    #[test]
    fn system_settings_do_not_expose_etc_or_private_keys() {
        let profile = profile(&Common::default(), &Macos::default());
        assert!(!profile.contains("(allow ipc-posix"));
        assert!(!profile.contains("(subpath \"/private/etc\")"));
        assert!(profile.contains("(subpath \"/private/etc/hosts\")"));
        assert!(profile.contains("(subpath \"/private/etc/ssl\")"));
        assert!(profile.contains("^/private/etc/(.*/)?private(/|$)"));
        for secret in [
            "shadow",
            "master.passwd",
            "ssh_host_rsa_key",
            "environment",
            "machine-id",
        ] {
            assert!(
                !profile.contains(&format!("/private/etc/{secret}\"")),
                "{secret}"
            );
        }
        let grant = PathBuf::from("/private/etc/explicit-settings");
        let profile = profile_with_read(grant);
        let deny = profile.find("^/private/etc/(.*/)?private").unwrap();
        let explicit = profile
            .find("(subpath \"/private/etc/explicit-settings\")")
            .unwrap();
        assert!(deny < explicit);
    }

    fn profile_with_read(path: PathBuf) -> String {
        profile(
            &Common {
                read: vec![path],
                ..Default::default()
            },
            &Macos::default(),
        )
    }
}

#[cfg(test)]
mod exec_tests;
