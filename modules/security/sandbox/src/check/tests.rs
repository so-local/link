use std::collections::BTreeSet;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

use link_sandbox_client::{Confinement, Net, Programs};
use link_sandbox_confine::Os;

use super::probes::{internet, refused, services};
use super::*;

fn bait() -> Bait {
    Bait {
        home: "/nonexistent-home".into(),
        writable: vec![
            "/nonexistent-home/Link/Agents/main/home".into(),
            "/f".into(),
        ],
        secret: "s".into(),
        canary: "/c".into(),
        write: "/w".into(),
        settings: "/s".into(),
        hidden: vec![(
            "/nonexistent-home/Link/Agents".into(),
            "/nonexistent-home/Link/Agents/lnk-check".into(),
        )],
        reads: vec![
            (
                "/nonexistent-home/Link/Teams/research/shared".into(),
                "/nonexistent-home/Link/Teams/research/shared/lnk-check".into(),
                true,
            ),
            (
                "/nonexistent-home/Link/Teams/press/shared".into(),
                "/nonexistent-home/Link/Teams/press/shared/lnk-check".into(),
                false,
            ),
        ],
        elsewhere: "/e".into(),
        port: 1,
        lan: Some([192, 168, 1, 2].into()),
        pid: 1,
        keychain: Some("k".into()),
        abstract_socket: Some("a".into()),
        python: true,
        ports: vec![11434, 3000],
        sandbox: "/opt/lnk/lnk-sandbox".into(),
        models: vec![11434],
        payload: None,
        loader: None,
        lib: None,
        lan_hosts: Vec::new(),
        internet: true,
        dns: false,
        dot_files: Vec::new(),
        tag: 1,
    }
}

/// What a confinement grants beside its folders.
#[derive(Clone, Copy, PartialEq)]
enum Grant {
    Network,
    Localhost,
    ReadHome,
    DeveloperTools,
    Open,
}
use Grant::*;

/// A sandbox granted `grants`: with no proxy, `Network` is the host's
/// network (`Localhost` every port of it); behind one (`proxied`, or
/// `Open`, whose pages go through it), the internet.
fn sandbox(grants: &[Grant], proxied: bool) -> Confinement {
    let proxy = link_sandbox_client::Proxy {
        no_internet: !grants.contains(&Network),
        ..Default::default()
    };
    Confinement {
        name: "x".into(),
        write: Vec::new(),
        write_dot_files: Vec::new(),
        read: Vec::new(),
        read_home: grants.contains(&ReadHome),
        hide: Vec::new(),
        tmp_prefix: None,
        terminal: false,
        commands: Programs::Any,
        developer_tools: grants.contains(&DeveloperTools),
        builds: Vec::new(),
        net: match (proxied || grants.contains(&Open), grants.contains(&Network)) {
            (true, _) => Net::Proxied {
                proxy,
                ports: BTreeSet::new(),
            },
            (false, true) => Net::Host {
                ports: BTreeSet::new(),
                localhost: grants.contains(&Localhost),
            },
            (false, false) => Net::Closed,
        },
    }
}

fn expect(grants: &[Grant], linux: bool, what: &str) -> Expect {
    let os = match linux {
        true => Os::Linux,
        false => Os::MacOs,
    };
    probes(&sandbox(grants, false), &bait(), os, None)
        .into_iter()
        .find(|p| p.what.starts_with(what))
        .unwrap()
        .expect
}

#[test]
fn expectations_follow_the_permissions() {
    assert_eq!(expect(&[], false, "read a file"), Expect::Blocked);
    assert_eq!(expect(&[ReadHome], false, "read a file"), Expect::Allowed);
    assert_eq!(expect(&[ReadHome], false, "write in"), Expect::Blocked);
    assert_eq!(expect(&[ReadHome], true, "read Link's"), Expect::Blocked);
    assert_eq!(
        expect(&[ReadHome], true, "read a file outside"),
        Expect::Blocked
    );
    assert_eq!(expect(&[], false, "reach the internet"), Expect::Blocked);
    assert_eq!(expect(&[Network], false, "reach the"), Expect::Allowed);
    assert_eq!(
        expect(&[Network], false, "connect to another"),
        Expect::Blocked
    );
    assert!(matches!(
        expect(&[Network], true, "connect to another"),
        Expect::Gap(_)
    ));
    assert!(matches!(
        expect(&[Network], false, "connect to it at"),
        Expect::Gap(_)
    ));
    assert_eq!(expect(&[], false, "connect to it at"), Expect::Blocked);
    assert!(matches!(
        expect(&[Network], false, "connect to it as"),
        Expect::Gap(_)
    ));
    assert_eq!(expect(&[], false, "connect to it as"), Expect::Blocked);
    // Localhost alone, with no network, opens nothing.
    assert_eq!(
        expect(&[Localhost], true, "connect to another"),
        Expect::Blocked
    );
    assert_eq!(
        expect(&[Network, Localhost], true, "connect to another"),
        Expect::Allowed
    );
    // `open` opens a page through the proxy, never an app.
    assert_eq!(expect(&[Open], false, "open an app"), Expect::Blocked);
    assert!(matches!(
        expect(&[Network], true, "connect to an abstract"),
        Expect::Gap(_)
    ));
    assert_eq!(expect(&[], true, "connect to an abstract"), Expect::Blocked);
    // The kernel's risky calls: refused whatever is allowed (Linux).
    let all = [Network, ReadHome, Localhost, DeveloperTools];
    assert_eq!(expect(&all, true, "set up io_uring"), Expect::Blocked);
    assert_eq!(expect(&all, true, "make a user namespace"), Expect::Blocked);
}

#[test]
fn through_the_proxy_the_network_gaps_close() {
    let all = probes(
        &sandbox(&[Network, Localhost], true),
        &bait(),
        Os::Linux,
        Some("example.com"),
    );
    let find = |what: &str| {
        all.iter()
            .find(|p| p.what.starts_with(what))
            .unwrap()
            .expect
    };
    assert_eq!(find("connect to another"), Expect::Blocked);
    assert_eq!(find("connect to it at"), Expect::Blocked);
    assert_eq!(find("connect to it as"), Expect::Blocked);
    assert_eq!(find("reach the internet"), Expect::Allowed);
    assert_eq!(find("ask its proxy for the server"), Expect::Blocked);
    assert_eq!(find("ask its proxy for this machine's"), Expect::Blocked);
    assert_eq!(find("connect to an abstract"), Expect::Blocked);
    assert_eq!(find("ask its proxy for a host not"), Expect::Blocked);
    // Each IPv6 form of this machine, a metadata service and a private
    // address: refused, as the IPv4 address it holds.
    let forms: Vec<_> = all
        .iter()
        .filter(|p| p.what.contains("IPv6"))
        .map(|p| (p.arg.clone().unwrap(), p.expect))
        .collect();
    assert_eq!(forms.len(), 12);
    assert!(forms.iter().all(|(_, e)| *e == Expect::Blocked));
    for target in [
        "[::ffff:127.0.0.1]:1",
        "[64:ff9b::7f00:1]:1",
        "[::7f00:1]:1",
        "[2002:7f00:1::1]:1",
        "[64:ff9b::a9fe:a9fe]:80",
        "[2002:aff:ff01::1]:80",
    ] {
        assert!(forms.iter().any(|(t, _)| t == target), "{target}");
    }
    // A private address its [lan] lists is its own to reach.
    let lan = Bait {
        lan_hosts: vec!["10.0.0.0/8".into()],
        ..bait()
    };
    assert_eq!(ipv6_forms(&lan).len(), 8);
    // The one gap left: a port it may reach (a local model's), relayed
    // in with its whole API.
    let gaps: Vec<_> = all
        .iter()
        .filter(|p| matches!(p.expect, Expect::Gap(_)))
        .map(|p| p.what.as_str())
        .collect();
    assert_eq!(gaps, ["connect to port 3000 here, which it may reach"]);
    // A model's port: its admin API is tried, and must be refused.
    assert_eq!(find("use port 11434's admin API"), Expect::Blocked);
}

#[test]
fn every_connection_out_is_tried() {
    // Its allowed ports: reachable with their whole API, a known gap.
    assert!(matches!(
        expect(&[], true, "connect to port 3000"),
        Expect::Gap(_)
    ));
    // macOS services: those it gets, those a permission opens, and
    // those it never may.
    assert_eq!(expect(&[], false, "reach macOS's log"), Expect::Allowed);
    // The Keychain: whatever it's allowed.
    let all = [Network, DeveloperTools, Open, ReadHome, Localhost];
    assert_eq!(expect(&all, false, "reach the Keychain"), Expect::Blocked);
    assert_eq!(
        expect(&[Open], false, "reach LaunchServices"),
        Expect::Blocked
    );
    for never in [
        "reach the clipboard",
        "reach the screen",
        "send Apple Events",
    ] {
        assert_eq!(expect(&all, false, never), Expect::Blocked, "{never}");
    }
    // Linux has no such services.
    let linux = probes(&sandbox(&[], false), &bait(), Os::Linux, None);
    assert!(!linux.iter().any(|p| p.what.contains("clipboard")));
}

/// macOS's certificate checks (trustd): with the host's network, or
/// behind a proxy that reaches the internet; its network's settings
/// (configd) only with the host's.
#[test]
fn certificate_checks_come_with_the_internet() {
    let found = |sandbox: &Confinement, name: &str| {
        services(sandbox)
            .into_iter()
            .find(|(_, service, _)| *service == name)
            .unwrap()
            .2
    };
    let trustd = "com.apple.trustd";
    let configd = "com.apple.SystemConfiguration.configd";
    assert_eq!(found(&sandbox(&[Network], true), trustd), Expect::Allowed);
    assert_eq!(found(&sandbox(&[], true), trustd), Expect::Blocked);
    assert_eq!(found(&sandbox(&[Network], false), trustd), Expect::Allowed);
    assert_eq!(found(&sandbox(&[], false), trustd), Expect::Blocked);
    assert_eq!(found(&sandbox(&[Network], true), configd), Expect::Blocked);
    assert_eq!(found(&sandbox(&[Network], false), configd), Expect::Allowed);
}

#[test]
fn each_folder_it_may_write_is_tried() {
    let all = probes(&sandbox(&[], false), &bait(), Os::Linux, None);
    let writes: Vec<_> = all
        .iter()
        .filter(|p| p.expect == Expect::Allowed && p.lands.is_some())
        .map(|p| (p.what.as_str(), p.lands.clone().unwrap()))
        .collect();
    assert_eq!(
        writes,
        vec![
            (
                "write ~/Link/Agents/main/home",
                PathBuf::from("/nonexistent-home/Link/Agents/main/home/lnk-check")
            ),
            ("write /f", PathBuf::from("/f/lnk-check")),
        ]
    );
}

#[test]
fn a_folder_asked_for_is_read_and_one_refused_isnt() {
    let all = probes(&sandbox(&[], false), &bait(), Os::Linux, None);
    let own = all
        .iter()
        .find(|p| p.what == "read: ~/Link/Teams/research/shared")
        .expect("its team's folder is tried");
    assert_eq!((own.expect, own.required), (Expect::Allowed, true));
    let other = all
        .iter()
        .find(|p| p.what == "read what isn't its own: ~/Link/Teams/press/shared")
        .expect("another team's folder is tried");
    assert_eq!(other.expect, Expect::Blocked);
    assert!(other.path.as_ref().unwrap().ends_with("lnk-check"));
}

#[test]
fn each_hidden_folder_is_tried_whatever_is_allowed() {
    let all = probes(&sandbox(&[ReadHome], false), &bait(), Os::Linux, None);
    let hidden = all
        .iter()
        .find(|p| p.what == "read what's hidden: ~/Link/Agents")
        .unwrap();
    assert_eq!(hidden.expect, Expect::Blocked);
    assert_eq!(
        hidden.path.as_deref(),
        Some(Path::new("/nonexistent-home/Link/Agents/lnk-check"))
    );
}

#[test]
fn a_wildcard_over_a_providers_shared_domain_is_warned_of() {
    let tcp: Vec<String> = [
        "*.mongodb.net:27017",
        "*.abc12.mongodb.net:27017",
        "*.us-east-1.rds.amazonaws.com:5432",
        "*.mydb.abc123.us-east-1.rds.amazonaws.com:5432",
        "db.mongodb.net:27017",
        "*.example.com:5432",
    ]
    .map(String::from)
    .to_vec();
    let warned: Vec<_> = shared_wildcards(&tcp).into_iter().map(|(e, _)| e).collect();
    assert_eq!(
        warned,
        ["*.mongodb.net:27017", "*.us-east-1.rds.amazonaws.com:5432"]
    );
}

#[test]
fn only_reaching_what_is_blocked_is_a_leak() {
    let probe = |expect| Probe {
        what: String::new(),
        script: "",
        expect,
        lands: None,
        arg: None,
        path: None,
        required: false,
    };
    assert_eq!(probe(Expect::Blocked).verdict(true), Verdict::Leak);
    assert_eq!(probe(Expect::Blocked).verdict(false), Verdict::Blocked);
    assert_eq!(
        probe(Expect::Allowed).verdict(false),
        Verdict::AllowedFailed
    );
    assert_eq!(probe(Expect::Gap("g")).verdict(true), Verdict::Gap("g"));
}

/// A proxy's 502 is reaching an address it took, which didn't answer,
/// but not a name it couldn't resolve: on a machine without DNS
/// (behind an HTTPS proxy alone), `[lan] *` refusing `example.com`.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_name_with_no_address_here_is_reached_only_by_connecting() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // A proxy answering every CONNECT as it does one it couldn't dial.
    std::thread::spawn(move || {
        for mut client in listener.incoming().flatten() {
            let mut head = [0; 512];
            let _ = std::io::Read::read(&mut client, &mut head);
            let _ = std::io::Write::write_all(
                &mut client,
                b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    let reached = |target: &str| {
        let probe = refused(target);
        let status = Command::new("/bin/bash")
            .args(["-c", probe.script])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HTTPS_PROXY", format!("http://127.0.0.1:{port}"))
            .env("LNK_PROBE_ARG", probe.arg.as_deref().unwrap())
            .status()
            .unwrap();
        probe.verdict(status.success())
    };
    // `.invalid` never resolves (RFC 6761).
    assert_eq!(reached("lnk-check.invalid:443"), Verdict::Blocked);
    assert_eq!(reached("10.255.255.1:80"), Verdict::Leak);
}

/// A sandbox with no internet whose proxy is slow to refuse, as on a
/// busy machine, is refused: a silence past the time its probe gives
/// dialing is never taken for the internet reached.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_slow_refusal_is_never_a_leak() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // A proxy refusing every CONNECT, later than the 3 seconds the probe
    // waits for one it lets through.
    std::thread::spawn(move || {
        for mut client in listener.incoming().flatten() {
            let mut head = [0; 512];
            let _ = std::io::Read::read(&mut client, &mut head);
            std::thread::sleep(std::time::Duration::from_millis(3500));
            let _ = std::io::Write::write_all(
                &mut client,
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    let probe = internet(false);
    let status = Command::new("/bin/bash")
        .args(["-c", probe.script])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HTTPS_PROXY", format!("http://127.0.0.1:{port}"))
        .env("LNK_PROBE_ARG", probe.arg.as_deref().unwrap())
        .status()
        .unwrap();
    assert_eq!(probe.verdict(status.success()), Verdict::Blocked);
}

/// What the sandbox planted where the probe moves a `.git` aside is
/// never put in place as the folder's `.git`: only a `.git` that was
/// there before the probes, moved aside by this run, comes back.
#[test]
fn a_planted_git_is_never_put_back() {
    let temp = link_test_kit::TempDir::new("check-git");
    let folder = temp.join("project");
    let planted = super::dot_files::moved_git(&folder, 7);
    std::fs::create_dir_all(planted.join("hooks")).unwrap();
    std::fs::write(
        planted.join("config"),
        "[core]\n\tfsmonitor = touch PWNED\n",
    )
    .unwrap();
    super::run::put_back_git(&folder, 7, false);
    assert!(
        !folder.join(".git").exists(),
        "a planted .git was put in place"
    );
    // One that was there and the sandbox moved aside: back.
    let moved = temp.join("moved");
    std::fs::create_dir_all(super::dot_files::moved_git(&moved, 7)).unwrap();
    super::run::put_back_git(&moved, 7, true);
    assert!(moved.join(".git").is_dir());
}
