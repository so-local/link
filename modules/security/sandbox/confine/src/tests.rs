use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use link_sandbox_client::{Confinement, Net};

use crate::linux::bwrap::{self, Blocked, apparmor_commands, apparmor_profile};
use crate::linux::etc;
use crate::macos::refusals;
use crate::macos::seatbelt::{literal, tmp_filter};
use crate::resolve;
use crate::{Common, Linux, Macos, Rules};

fn bwrap_args(bwrap: &str, rules: &Rules, argv: &[String]) -> Vec<String> {
    bwrap::args(bwrap, &rules.common, &rules.linux, argv)
}

/// The Seatbelt profile for a whole [`Rules`], the parts macOS enforces.
fn seatbelt_profile(rules: &Rules) -> String {
    crate::macos::seatbelt::profile(&rules.common, &rules.macos)
}

/// What a confinement grants beside its folders, each one of
/// [`Rules`]' own.
#[derive(Clone, Copy, PartialEq)]
enum Grant {
    Network,
    Localhost,
    ReadHome,
    DeveloperTools,
}

/// A confinement of these folders and no more.
fn confinement(
    write: Vec<PathBuf>,
    read: Vec<PathBuf>,
    hide: Vec<PathBuf>,
    write_dot_files: Vec<PathBuf>,
) -> Confinement {
    Confinement {
        name: "x".into(),
        write,
        write_dot_files,
        read,
        read_home: false,
        hide,
        tmp_prefix: None,
        terminal: false,
        commands: link_sandbox_client::Programs::Any,
        developer_tools: false,
        builds: Vec::new(),
        net: Net::Closed,
    }
}

fn policy(grants: &[Grant]) -> Rules {
    let ports = BTreeSet::from([18789, 11434]);
    Rules {
        common: Common {
            write: vec![
                "/Users/me/.lnk/data/agents/main/harnesses/otter".into(),
                "/Users/me/Link/Agents/main/home".into(),
            ],
            read: vec!["/opt/tools".into()],
            home: "/Users/me".into(),
            read_home: grants.contains(&Grant::ReadHome),
            hide: vec!["/srv/lnk-config".into()],
            programs: Vec::new(),
            closed: Vec::new(),
            outside: Vec::new(),
            commands: None,
        },
        linux: Linux {
            terminal: false,
            host: grants.contains(&Grant::Network),
            ports: ports.clone(),
            own: Vec::new(),
            confine: None,
            etc_hosts: None,
            resolv_conf: None,
            caps: Default::default(),
        },
        macos: Macos {
            tmp_prefix: Some("otter".into()),
            network: grants.contains(&Grant::Network),
            localhost: grants.contains(&Grant::Localhost),
            ports,
            serves: Default::default(),
            certificates: false,
            developer_tools: grants.contains(&Grant::DeveloperTools),
            report_execs: false,
        },
    }
}

#[test]
fn localhost_is_its_own_ports_unless_allowed() {
    let alone = seatbelt_profile(&policy(&[]));
    assert!(alone.contains(r#"(allow network-outbound (remote ip "localhost:18789"))"#));
    assert!(alone.contains(r#"(allow network-outbound (remote ip "localhost:11434"))"#));
    assert!(!alone.contains(r#"(remote ip "localhost:*")"#));
    // With the internet, localhost is denied again after "*:*", then
    // its ports re-allowed: the last matching rule wins.
    let online = seatbelt_profile(&policy(&[Grant::Network]));
    let all = online
        .find(r#"(allow network-outbound (remote ip "*:*"))"#)
        .unwrap();
    let deny = online
        .find(r#"(deny network-outbound (remote ip "localhost:*"))"#)
        .unwrap();
    let own = online.find(r#"(remote ip "localhost:18789")"#).unwrap();
    assert!(all < deny && deny < own, "{online}");
    assert!(!online.contains("ip6"), "ip6 matches IPv4 too");
    let open = seatbelt_profile(&policy(&[Grant::Network, Grant::Localhost]));
    assert!(open.contains(r#"(allow network-outbound (remote ip "localhost:*"))"#));
    assert!(!open.contains("(deny network-outbound"));
}

#[test]
fn a_file_it_reads_opens_that_file_only() {
    // This program, as the proxied and checked policies read it: the
    // file, never the folder it's in.
    let exe = std::env::current_exe().unwrap();
    let mut rules = policy(&[]);
    rules.common.read.push(exe.clone());
    let profile = seatbelt_profile(&rules);
    let exe = literal(&exe.display().to_string());
    assert!(
        profile.contains(&format!("\n  (literal {exe})")),
        "{profile}"
    );
    assert!(!profile.contains(&format!("(subpath {exe})")), "{profile}");
    assert!(profile.contains(r#"(subpath "/opt/tools")"#), "{profile}");
    // Linux: bubblewrap mounts the confining program alone.
    rules.linux.confine = Some("/home/me/.lnk/exec/bin/lnk-sandbox".into());
    let args = bwrap_args("bwrap", &rules, &["true".into()]).join(" ");
    assert!(
        args.contains(
            "--ro-bind-try /home/me/.lnk/exec/bin/lnk-sandbox /home/me/.lnk/exec/bin/lnk-sandbox"
        ),
        "{args}"
    );
    assert!(!args.contains("/home/me/.lnk/exec/bin "), "{args}");
}

#[test]
fn the_default_profile_is_the_box_and_localhost() {
    let profile = seatbelt_profile(&policy(&[]));
    assert!(profile.contains("(deny default)"));
    let writes = profile.split("(allow file-write*").nth(1).unwrap();
    assert!(writes.contains(r#"(subpath "/Users/me/Link/Agents/main/home")"#));
    assert!(writes.contains(r#"(regex #"^/private/tmp/\.?otter([-._/]|$)")"#));
    assert!(!writes.contains("/opt/tools"));
    // The home folder: directories can be stat()ed, files can't.
    assert!(profile.contains("(allow file-read-metadata (vnode-type DIRECTORY))"));
    assert!(!profile.contains(r#"(subpath "/Users/me")"#));
    assert!(!profile.contains("(allow file-read-metadata)"));
    // dyld needs the root; unix sockets use their own filter.
    assert!(profile.contains(r#"(allow file-read* (literal "/"))"#));
    assert!(profile.contains(r#"(literal "/tmp") (literal "/var") (literal "/etc")"#));
    assert!(profile.contains(
        r#"(allow network-bind (local unix-socket (subpath "/Users/me/.lnk/data/agents/main/harnesses/otter")"#
    ));
    assert!(profile.contains(r#"(path-regex #"^/private/tmp/\.?otter([-._/]|$)")"#));
    // No broad sysctl-read: kern.procargs2 gives other processes' env.
    assert!(!profile.contains("(allow sysctl-read)"));
    assert!(profile.contains(r#"(sysctl-name "kern.osversion")"#));
    // libuv's uptime (Node): a Node harness dies without it.
    assert!(profile.contains(r#"(sysctl-name "kern.boottime")"#));
    assert!(!profile.contains("(sysctl-name \"kern.procargs"));
    // The system's own folders, never /System/Volumes, which holds the
    // Data volume, a second path to /Users.
    assert!(!profile.contains(r#"(subpath "/System")"#));
    assert!(profile.contains(r#"(subpath "/System/Library")"#));
    assert!(!profile.contains(r#"(subpath "/System/Volumes")"#));
    assert!(!profile.contains("/System/Volumes/Data"));
    // A program's temp files end at a separator, and the locale's
    // settings are the two Library folders' alone.
    assert!(profile.contains(r#"#"^/private/tmp/\.?otter([-._/]|$)")"#));
    assert!(profile.contains(r#"(regex #"^/Library/Preferences/\.GlobalPreferences"#));
    assert!(!profile.contains(r#"(regex #"/Library/Preferences"#));
    // Neither every process nor the boot session's ID.
    assert!(!profile.contains(r#"(sysctl-name-prefix "kern.proc.all")"#));
    assert!(!profile.contains(r#"(sysctl-name "kern.bootsessionuuid")"#));
    assert!(!profile.contains("(allow network*)"));
    for closed in [
        "SecurityServer",
        "launchservicesd",
        "DNSConfiguration",
        "/Library/Developer",
    ] {
        assert!(!profile.contains(closed), "{closed} is open by default");
    }
    assert_eq!(tmp_filter(Some("../x"), "regex"), None);
}

#[test]
fn each_permission_opens_its_own_part() {
    let with = |p| seatbelt_profile(&policy(&[p]));
    assert!(with(Grant::Network).contains(r#"(remote ip "*:*")"#));
    assert!(with(Grant::DeveloperTools).contains("/Library/Developer"));
    assert!(
        with(Grant::DeveloperTools)
            .contains(r#"(allow user-preference-read (preference-domain "com.apple.dt.Xcode"))"#)
    );
    assert!(with(Grant::ReadHome).contains(r#"(allow file-read* (subpath "/Users/me"))"#));
    assert!(!with(Grant::Network).contains("SecurityServer"));
    assert!(with(Grant::Network).contains("com.apple.trustd"));
}

#[test]
fn missing_folders_resolve_through_their_parent() {
    let root = link_test_kit::TempDir::new("resolve");
    let real = root.canonicalize().unwrap();
    assert_eq!(resolve(&root.join("a/b")), real.join("a/b"));
    assert!(!root.join("a").exists(), "resolving creates nothing");
}

/// On Linux every sandbox starts with its first program, which closes
/// what it was handed, whatever the architecture:
/// only the seccomp filter is [`FILTERED`]'s.
#[test]
fn every_linux_sandbox_starts_with_its_first_program() {
    let rules = Rules::of(&confinement(Vec::new(), Vec::new(), Vec::new(), Vec::new())).unwrap();
    assert_eq!(rules.linux.confine.is_some(), cfg!(target_os = "linux"));
}

/// A link `at` a non-root user owns, to `target`: made here, and where
/// this user is root, handed to `nobody` so it reads as another user's.
/// False where no such link can be made.
#[cfg(unix)]
fn non_root_link(at: &Path, target: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::os::unix::fs::symlink(target, at).unwrap();
    if std::fs::symlink_metadata(at).unwrap().uid() == 0
        && std::os::unix::fs::lchown(at, Some(65534), None).is_err()
    {
        return false;
    }
    std::fs::symlink_metadata(at).unwrap().uid() != 0
}

/// A grant reached through a link a non-root user controls is refused,
/// wherever the link sits: a team's shared folder a program writes, where
/// it could plant one to reach `~/.ssh`; the grant's own root, where it
/// could swap the whole folder; a link granted itself. A folder really
/// there is granted as it is.
#[cfg(unix)]
#[test]
fn a_grant_through_a_non_root_link_is_refused() {
    let planted = link_test_kit::TempDir::new("planted");
    let root = resolve(&planted);
    let (teams, ssh) = (root.join("Teams"), root.join(".ssh"));
    let shared = teams.join("t");
    std::fs::create_dir_all(shared.join("notes")).unwrap();
    std::fs::create_dir_all(&ssh).unwrap();
    if !non_root_link(&shared.join("drafts"), &ssh) {
        link_test_kit::gate::lacks(
            "a link another user owns",
            "root here can't hand a link to nobody (lchown is refused)",
            None,
        );
        return;
    }
    assert!(non_root_link(&shared.join("up"), &root));
    let rules = |write: Vec<PathBuf>, read: Vec<PathBuf>, hide: Vec<PathBuf>| {
        Rules::of(&confinement(write, read, hide, Vec::new()))
    };
    // A planted link inside a hidden shared folder, to a secret or up and
    // around: refused, not quietly dropped or followed.
    assert!(rules(vec![shared.join("drafts")], Vec::new(), vec![teams.clone()]).is_err());
    assert!(
        rules(
            vec![shared.join("up/.ssh")],
            Vec::new(),
            vec![teams.clone()]
        )
        .is_err()
    );
    assert!(rules(Vec::new(), vec![shared.join("drafts")], Vec::new()).is_err());
    // A folder really there is granted as it is, link beside it or not.
    let ok = rules(vec![shared.join("notes")], Vec::new(), vec![teams]).unwrap();
    assert_eq!(ok.common.write, vec![shared.join("notes")]);
}

#[test]
fn literals_are_escaped() {
    assert_eq!(literal("/a \"b\"\\c"), "\"/a \\\"b\\\"\\\\c\"");
}

#[test]
fn on_macos_refusals_are_read_from_outside_and_learning_reports_each_program() {
    let outer = refusals(
        Path::new("/opt/lnk/lnk-sandbox"),
        &["/bin/sh".into(), "/usr/libexec/".into()],
        vec![
            "/usr/bin/sandbox-exec".into(),
            "-f".into(),
            "p.sb".into(),
            "sh".into(),
        ],
    );
    assert_eq!(
        outer.join(" "),
        "/opt/lnk/lnk-sandbox refusals --exec /bin/sh --exec /usr/libexec/ -- \
         /usr/bin/sandbox-exec -f p.sb sh"
    );
    let mut learning = policy(&[]);
    learning.macos.report_execs = true;
    let profile = seatbelt_profile(&learning);
    assert!(
        profile.contains("(allow process-exec (with report))"),
        "{profile}"
    );
    assert!(!seatbelt_profile(&policy(&[])).contains("with report"));
}

#[test]
fn a_projects_dot_files_are_read_only_but_where_opened() {
    let root = link_test_kit::TempDir::new("rules-dots");
    let app = root.join("app");
    let docs = root.join("docs");
    std::fs::create_dir_all(app.join(".git")).unwrap();
    std::fs::create_dir_all(&docs).unwrap();
    let (app, docs) = (resolve(&app), resolve(&docs));
    let policy = confinement(
        vec![app.clone(), docs.clone(), app.join(".vscode/out")],
        Vec::new(),
        Vec::new(),
        vec![docs.clone()],
    );
    let rules = Rules::of(&policy).unwrap();
    let at: Vec<PathBuf> = rules.common.outside.iter().map(|e| e.path()).collect();
    // The app's, not the docs', which it opens.
    assert!(at.contains(&app.join(".git")) && at.contains(&app.join(".idea")));
    assert!(!at.iter().any(|p| p.starts_with(&docs)), "{at:?}");
    // macOS: refused by path, any case; a folder it writes in one,
    // open again after.
    let profile = seatbelt_profile(&rules);
    let deny = profile.find("(deny file-write*\n  (regex").unwrap();
    assert!(profile.contains("/app/\\.[gG][iI][tT](/|$)"), "{profile}");
    let again = format!(
        "(allow file-write* (subpath {}))",
        literal(&app.join(".vscode/out").display().to_string())
    );
    assert!(profile.find(&again).unwrap() > deny, "{profile}");
    // Linux: read only over the folder's own bind, after it.
    let args = bwrap_args("/usr/bin/bwrap", &rules, &["true".into()]);
    let git = app.join(".git").display().to_string();
    let ro = args
        .windows(3)
        .position(|w| w[0] == "--ro-bind" && w[1] == git)
        .unwrap();
    let app_text = app.display().to_string();
    let bound = args
        .windows(2)
        .position(|w| w[0] == "--bind-try" && w[1] == app_text)
        .unwrap();
    assert!(ro > bound);
    // The folder it writes in .vscode is bound again, after.
    let out = app.join(".vscode/out").display().to_string();
    let vscode = app.join(".vscode").display().to_string();
    let covered = args
        .windows(2)
        .position(|w| w[0] == "--ro-bind" && w[1] == vscode)
        .unwrap();
    let rebound = args
        .windows(2)
        .rposition(|w| w[0] == "--bind-try" && w[1] == out)
        .unwrap();
    assert!(rebound > covered);
}

#[test]
fn a_dot_file_in_links_data_folders_is_bound_only_while_its_there() {
    let root = link_test_kit::TempDir::new("rules-dots-swept");
    let caches = root.join("caches");
    std::fs::create_dir_all(caches.join(".git")).unwrap();
    let caches = resolve(&caches);
    let policy = confinement(vec![caches.clone()], Vec::new(), Vec::new(), Vec::new());
    let mut rules = Rules::of(&policy).unwrap();
    let git = caches.join(".git").display().to_string();
    let flag = |rules: &Rules| {
        let args = bwrap_args("/usr/bin/bwrap", rules, &["true".into()]);
        args.windows(2)
            .find(|w| w[1] == git)
            .map(|w| w[0].clone())
            .unwrap()
    };
    assert_eq!(flag(&rules), "--ro-bind");
    // Another run's sweep may remove it before this one starts.
    for entry in &mut rules.common.outside {
        entry.swept = true;
    }
    assert_eq!(flag(&rules), "--ro-bind-try");
}

#[test]
fn ubuntus_namespace_rule_is_told_apart() {
    let denied = "bwrap: setting up uid map: Permission denied";
    assert_eq!(bwrap::blocked(true, denied), Blocked::AppArmor);
    assert_eq!(
        bwrap::blocked(true, "bwrap: No permissions to create a new namespace"),
        Blocked::AppArmor
    );
    // The same words without the rule, or another failure with it.
    assert!(matches!(bwrap::blocked(false, denied), Blocked::Other(_)));
    assert!(matches!(
        bwrap::blocked(true, "bwrap: execvp: nope"),
        Blocked::Other(_)
    ));
}

#[test]
fn roots_try_doesnt_speak_for_users_under_ubuntus_rule() {
    assert!(bwrap::needs_profile(true, true, false));
    // Already named, the rule off, or tried as a user: the try stands.
    assert!(!bwrap::needs_profile(true, true, true));
    assert!(!bwrap::needs_profile(true, false, false));
    assert!(!bwrap::needs_profile(false, true, false));
}

#[test]
fn the_apparmor_profile_only_adds_user_namespaces_to_bwrap() {
    let profile = apparmor_profile("/usr/bin/bwrap");
    assert!(profile.contains("profile lnk-bwrap /usr/bin/bwrap flags=(unconfined) {\n  userns,\n"));
    assert!(profile.starts_with("# Written by lnk"));
    assert!(profile.ends_with("}\n"));
    let commands = apparmor_commands("/usr/bin/bwrap");
    assert!(commands.contains("sudo tee /etc/apparmor.d/lnk-bwrap"));
    assert!(commands.ends_with("sudo apparmor_parser -r /etc/apparmor.d/lnk-bwrap"));
}

#[test]
fn bwrap_run_by_root_drops_its_capabilities_and_unpacks_archives() {
    let root = unsafe { libc::geteuid() } == 0;
    for no_userns in [false, true] {
        let mut policy = policy(&[]);
        policy.linux.caps.no_userns = no_userns;
        let args = bwrap_args("/usr/bin/bwrap", &policy, &["tar".into()]);
        let set = args
            .windows(3)
            .any(|w| w == ["--setenv", "TAR_OPTIONS", "--no-same-owner"]);
        assert_eq!(set, root, "{args:?}");
        let dropped = args.windows(2).any(|w| w == ["--cap-drop", "ALL"]);
        assert_eq!(dropped, root, "{args:?}");
        // A user namespace of its own, asked for by root (bubblewrap
        // makes one for anyone else unasked).
        let userns = args.contains(&"--unshare-user".to_string());
        assert_eq!(userns, root || no_userns, "{args:?}");
    }
}

#[test]
fn bwrap_hides_home_run_and_other_processes() {
    let joined = bwrap_args("/usr/bin/bwrap", &policy(&[]), &["node".into()]).join(" ");
    assert!(joined.contains("--unshare-pid"));
    assert!(joined.contains("--new-session"));
    let mut own = policy(&[]);
    own.linux.terminal = true;
    assert!(!bwrap_args("/usr/bin/bwrap", &own, &[]).contains(&"--new-session".into()));
    // The filter first inside, this program alone, read only;
    // no user namespaces where bubblewrap can refuse them.
    let mut walled = policy(&[]);
    walled.linux.confine = Some("/opt/lnk/lnk-sandbox".into());
    walled.linux.caps.no_userns = true;
    let joined = bwrap_args("/usr/bin/bwrap", &walled, &["node".into()]).join(" ");
    assert!(
        joined.ends_with(
            "--unshare-user --disable-userns --remount-ro /etc \
             --ro-bind-try /opt/lnk/lnk-sandbox /opt/lnk/lnk-sandbox -- \
             /opt/lnk/lnk-sandbox confine -- node"
        ),
        "{joined}"
    );
    assert!(joined.contains("--tmpfs /run"));
    assert!(joined.contains("--unshare-net"));
    assert!(joined.contains("--tmpfs /Users/me --ro-bind-try /opt/tools"));
    assert!(
        joined
            .contains("--bind-try /Users/me/Link/Agents/main/home /Users/me/Link/Agents/main/home")
    );
    assert!(joined.ends_with("-- node"));
    // The system folders, not the whole root; Link's settings hidden.
    assert!(!joined.contains("--ro-bind / /"), "{joined}");
    if Path::new("/usr").is_dir() {
        assert!(joined.contains("--ro-bind /usr /usr"), "{joined}");
    }
    // Not the whole of /etc.
    assert!(!joined.contains("--ro-bind /etc /etc"), "{joined}");
    assert!(joined.contains("--tmpfs /srv/lnk-config"));
    let open = policy(&[Grant::Network, Grant::ReadHome]);
    let open = bwrap_args("/usr/bin/bwrap", &open, &[]).join(" ");
    assert!(!open.contains("--unshare-net") && !open.contains("--tmpfs /Users/me"));
    let at = |text| {
        open.find(text)
            .unwrap_or_else(|| panic!("{text} in {open}"))
    };
    assert!(at("--ro-bind-try /Users/me /Users/me") < at("--tmpfs /srv/lnk-config"));
}

/// Of `/etc`, only what's listed, a link kept as one, and anything
/// named `private` in it never shown, a link so named too.
#[test]
fn of_etc_only_what_programs_need_is_shown() {
    let dir = link_test_kit::TempDir::new("etc");
    for folder in [
        "ssl/private",
        "ssl/certs",
        "pki/tls/certs",
        "pki/keys",
        "java-21-openjdk",
        "ssh",
        "nix",
    ] {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
    }
    for file in [
        "passwd",
        "shadow",
        "machine-id",
        "environment",
        "gitconfig",
        "wgetrc",
        "nix/nix.conf",
        "ssl/private/host.key",
        "ssl/certs/ca.pem",
        "pki/keys/key.pem",
        "ssh/ssh_host_ed25519_key",
        "ssh/ssh_config",
    ] {
        std::fs::write(dir.join(file), "x").unwrap();
    }
    // A link named `private`, to keys elsewhere in what's shown.
    std::os::unix::fs::symlink("../keys", dir.join("pki/tls/private")).unwrap();
    std::os::unix::fs::symlink("../run/resolve/stub-resolv.conf", dir.join("resolv.conf")).unwrap();
    let mut args = Vec::new();
    etc::args(&dir, &mut args);
    let joined = args.join(" ");
    let d = dir.display();
    assert!(joined.starts_with("--tmpfs /etc "), "{joined}");
    for shown in [
        format!("--ro-bind {d}/passwd /etc/passwd"),
        "--dir /etc/ssl".into(),
        format!("--ro-bind {d}/ssl/certs /etc/ssl/certs"),
        "--dir /etc/pki/tls".into(),
        format!("--ro-bind {d}/pki/tls/certs /etc/pki/tls/certs"),
        format!("--ro-bind {d}/java-21-openjdk /etc/java-21-openjdk"),
        format!("--ro-bind {d}/ssh/ssh_config /etc/ssh/ssh_config"),
        "--symlink ../run/resolve/stub-resolv.conf /etc/resolv.conf".into(),
    ] {
        assert!(joined.contains(&shown), "{shown} in {joined}");
    }
    for hidden in [
        "ssl/private",
        "pki/tls/private",
        "shadow",
        "machine-id",
        "environment",
        "gitconfig",
        "wgetrc",
        "nix",
        "ssh/ssh_host_ed25519_key",
    ] {
        let hidden = format!("/etc/{hidden}");
        assert!(
            !args
                .iter()
                .any(|arg| arg == &hidden || arg.starts_with(&format!("{hidden}/"))),
            "{hidden} in {joined}"
        );
    }
    assert!(!args.iter().any(|arg| arg == "/etc/ssh"), "{joined}");
}

/// `/etc` is made read only once every mount in it is made: nothing
/// is written there from inside, root's `ld.so.preload` among it.
#[test]
fn etc_is_read_only_inside() {
    let joined = bwrap_args("/usr/bin/bwrap", &policy(&[]), &["true".into()]).join(" ");
    let at = |text: &str| {
        joined
            .rfind(text)
            .unwrap_or_else(|| panic!("{text} in {joined}"))
    };
    assert!(at("--tmpfs /etc ") < at("--remount-ro /etc"), "{joined}");
    assert!(at("--remount-ro /etc") < at(" -- "), "{joined}");
}

#[test]
fn links_settings_open_only_to_the_sandboxs_own_folders() {
    let mut rules = policy(&[]);
    rules.common.write = vec!["/Users/me".into(), "/srv/lnk-config/sandbox/tools".into()];
    rules.common.read = vec!["/srv/lnk-config/sandbox".into()];
    rules.linux.own = vec![
        "/srv/lnk-config/agents/main/net/h".into(),
        "/run/lnk/agent/net/h".into(),
    ];
    // Linux: no grant in them is mounted over the folder hiding them,
    // but the sandbox's own, after it.
    let joined = bwrap_args("/usr/bin/bwrap", &rules, &[]).join(" ");
    assert!(!joined.contains("/srv/lnk-config/sandbox"), "{joined}");
    let at = |text| {
        joined
            .find(text)
            .unwrap_or_else(|| panic!("{text} in {joined}"))
    };
    assert!(
        !joined.contains("/srv/lnk-config/agents/main/net/h"),
        "{joined}"
    );
    assert!(
        at("--tmpfs /srv/lnk-config") < at("--bind-try /run/lnk/agent/net/h /run/lnk/agent/net/h")
    );
    // macOS: closed to writing too, after a write of the home holding
    // them.
    let profile = seatbelt_profile(&rules);
    let write = profile.find(r#"(subpath "/Users/me")"#).unwrap();
    let deny = profile
        .find(r#"(deny file-read* file-write* (subpath "/srv/lnk-config"))"#)
        .unwrap();
    assert!(write < deny, "{profile}");
}

/// A harness's home in Link's installation folder is the one grant there that
/// stays: bound over the empty folder hiding it (Linux), allowed after
/// the deny (macOS). The folder of homes and the rest stay closed.
#[test]
fn a_harness_home_in_links_folder_stays_granted() {
    let mut rules = policy(&[]);
    rules.common.hide = vec!["/srv/lnk/safe".into()];
    let home = "/srv/lnk/data/agents/main/harnesses/hermes";
    let proxy = "/srv/lnk/safe/agents/main/proxy";
    rules.common.write = vec![home.into(), proxy.into()];
    rules.common.read = vec!["/srv/lnk/safe/agents".into()];
    let joined = bwrap_args("/usr/bin/bwrap", &rules, &[]).join(" ");
    let at = |text: &str| {
        joined
            .find(text)
            .unwrap_or_else(|| panic!("{text} in {joined}"))
    };
    assert!(
        at(&format!("--bind-try {home} {home}")) < at("--tmpfs /srv/lnk/safe "),
        "{joined}"
    );
    assert!(!joined.contains(proxy), "{joined}");
    assert!(
        !joined.contains("--ro-bind-try /srv/lnk/safe/agents "),
        "{joined}"
    );
    let profile = seatbelt_profile(&rules);
    let deny = profile
        .find(r#"(deny file-read* file-write* (subpath "/srv/lnk/safe"))"#)
        .unwrap();
    let allow = profile.find(&format!(r#"(subpath "{home}")"#)).unwrap();
    assert!(allow < deny, "{profile}");
    assert!(
        !profile.contains(&format!(r#"(allow file-write* (subpath "{proxy}"))"#)),
        "{profile}"
    );
}

#[test]
fn what_is_hidden_stays_hidden_but_for_what_the_policy_names_in_it() {
    let root = link_test_kit::TempDir::new("hidden");
    let agents = root.join("Link/Agents");
    std::fs::create_dir_all(&agents).unwrap();
    let own = agents.join("main/Harnesses/otter");
    let mut rules = policy(&[Grant::ReadHome]);
    let settings = root.join("settings");
    std::fs::create_dir(&settings).unwrap();
    rules.common.hide = vec![settings.clone()];
    rules.common.write = vec![own.clone()];
    // A folder holding what's hidden: mounted first, then hidden over.
    rules.common.read = vec![root.to_path_buf(), "/srv".into()];
    rules.common.closed = vec![agents.clone(), root.join("missing")];
    let joined = bwrap_args("/usr/bin/bwrap", &rules, &[]).join(" ");
    let (root, agents, own) = (
        root.display().to_string(),
        agents.display().to_string(),
        own.display().to_string(),
    );
    let at = |text: &str| {
        joined
            .find(text)
            .unwrap_or_else(|| panic!("{text} in {joined}"))
    };
    assert!(at(&format!("--ro-bind-try {root} {root}")) < at(&format!("--tmpfs {agents}")));
    // Link's settings too: the granted root holds them.
    assert!(
        at(&format!("--ro-bind-try {root} {root}"))
            < at(&format!("--tmpfs {}", settings.display()))
    );
    assert!(at(&format!("--tmpfs {agents}")) < at(&format!("--bind-try {own} {own}")));
    // What doesn't exist has nothing to hide (and bubblewrap couldn't
    // mount over it in a read-only home).
    assert!(!joined.contains("missing"), "{joined}");

    let profile = seatbelt_profile(&rules);
    let at = |text: &str| {
        profile
            .find(text)
            .unwrap_or_else(|| panic!("{text} in {profile}"))
    };
    let deny = format!("(deny file-read* file-write* (subpath \"{agents}\"))");
    let reopen = format!("(allow file-write* (subpath \"{own}\"))\n");
    assert!(at("; read-home") < at(&deny));
    assert!(at(&deny) < profile.rfind(&reopen).unwrap());
    // Link's settings last, after anything reopened.
    assert!(
        profile.rfind(&reopen).unwrap()
            < at(&format!(
                "(deny file-read* file-write* (subpath \"{}\"))",
                settings.display()
            ))
    );
}

#[test]
fn devices_terminals_and_homebrew_data_stay_closed() {
    let profile = seatbelt_profile(&policy(&[]));
    assert!(!profile.contains(r#"(subpath "/dev")"#));
    assert!(!profile.contains("ttys"));
    assert!(profile.contains(r#"(literal "/dev/urandom")"#));
    assert!(!profile.contains(r#"(subpath "/opt/homebrew")"#));
    assert!(!profile.contains(r#"(subpath "/usr")"#));
    assert!(profile.contains(r#"(subpath "/opt/homebrew/Cellar")"#));
    assert!(!profile.contains("homebrew/var"));
    // Listening: localhost only, on its own ports, unless network.
    assert!(!profile.contains(r#"(local ip "localhost:*")"#));
    let mut own = policy(&[]);
    own.macos.serves = [18789].into();
    assert!(
        seatbelt_profile(&own)
            .contains(r#"(allow network-bind network-inbound (local ip "localhost:18789"))"#)
    );
    // A port it only reaches is never one it serves: no `[::1]:P` taken
    // beside a server on `127.0.0.1:P`.
    let reached = policy(&[]);
    assert!(reached.macos.ports.contains(&18789));
    assert!(!seatbelt_profile(&reached).contains("(local ip \"localhost:18789\")"));
    assert!(!profile.contains(r#"(local ip "*:*")"#));
    let online = seatbelt_profile(&policy(&[Grant::Network]));
    assert!(online.contains(r#"(allow network-bind network-inbound (local ip "*:*"))"#));
    // Link's settings stay closed with read-home, the rule after it.
    let home = seatbelt_profile(&policy(&[Grant::ReadHome]));
    let read = home
        .find(r#"(allow file-read* (subpath "/Users/me"))"#)
        .unwrap();
    let deny = home
        .find(r#"(deny file-read* file-write* (subpath "/srv/lnk-config"))"#)
        .unwrap();
    assert!(read < deny);
}

/// The mount points bubblewrap's `args` expose, by the flag that makes
/// each: a bind or symlink at its target (the last of its two paths), a
/// `--tmpfs`/`--dev`/`--proc`/`--dir` at its one path. Every other flag
/// is stepped over with its operands.
#[cfg(target_os = "linux")]
fn mount_targets(args: &[String]) -> BTreeSet<PathBuf> {
    let mut targets = BTreeSet::new();
    let mut i = 0;
    while i < args.len() {
        let (step, target) = match args[i].as_str() {
            "--ro-bind" | "--ro-bind-try" | "--bind" | "--bind-try" | "--symlink" => {
                (3, args.get(i + 2))
            }
            "--tmpfs" | "--dev" | "--proc" | "--dir" => (2, args.get(i + 1)),
            _ => (1, None),
        };
        if let Some(target) = target {
            targets.insert(PathBuf::from(target));
        }
        i += step;
    }
    targets
}

/// The drift guard. The Linux system baseline — the read-only system
/// folders, `/etc`, and the fresh writable folders — is one declaration
/// (`crate::system`) that two independent walls render: bubblewrap's
/// mounts and Landlock's second file wall. They must agree, and this holds
/// the dangerous direction: the file wall allows nothing the mounts do not
/// expose. A change that makes Landlock looser than the mounts — a stray
/// path in the wall, or a baseline mount dropped from bubblewrap — fails
/// here. (The per-policy grants cross into both from the same `Rules` and
/// cannot drift; this guards the baseline the two once encoded apart.)
#[cfg(target_os = "linux")]
#[test]
fn the_file_wall_is_a_subset_of_the_mounts() {
    let mounts = mount_targets(&bwrap_args(
        "/usr/bin/bwrap",
        &policy(&[]),
        &["true".into()],
    ));
    // The wall's baseline allow-set, with no per-policy grant.
    let (read, write) = crate::linux::enter::baseline();
    for path in read.iter().chain(&write) {
        let exposed = mounts.iter().any(|m| path == m || path.starts_with(m));
        // bubblewrap mounts a system read folder only where it is a folder
        // or a link on this machine; absent, both walls pass it by, so the
        // wall is no looser than the mounts there.
        let system_skipped = crate::system::SYSTEM.iter().any(|s| Path::new(s) == path)
            && !matches!(
                path.symlink_metadata(),
                Ok(meta) if meta.file_type().is_symlink() || meta.is_dir()
            );
        assert!(
            exposed || system_skipped,
            "the file wall allows {} but the mounts do not expose it: the second wall is looser \
             than the first",
            path.display()
        );
    }
    // The baseline the guard checks is not empty: a parser or rendering
    // that silently produced nothing would pass the loop vacuously.
    assert!(
        mounts.contains(Path::new("/etc")) && mounts.contains(Path::new("/tmp")),
        "the baseline mounts were not rendered: {mounts:?}"
    );
}

/// A socket opens as a handle (`O_PATH`), as `in.sock` must for the
/// harness's ports to answer: opened for reading, as musl's
/// `custom_flags` did, it fails with ENXIO.
#[cfg(target_os = "linux")]
#[test]
fn open_path_opens_a_socket_as_a_handle() {
    use std::os::fd::AsRawFd;
    let dir = link_test_kit::TempDir::new("open-path");
    let socket = dir.join("in.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let file = link_files::nofollow::open_path(&socket, libc::O_NOFOLLOW).unwrap();
    // SAFETY: an open file descriptor.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    assert_ne!(flags & libc::O_PATH, 0, "opened without O_PATH");
}
