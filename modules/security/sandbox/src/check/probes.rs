//! The probes: what each tries, and what it should find for the
//! confinement it runs under.

use link_sandbox_client::{Confinement, Net};
use link_sandbox_confine::Os;

use super::dot_files::dot_file_probes;
use super::scripts::{
    DNS_ANSWERED, KERNEL_PROBE, MODEL_ADMIN, MODEL_ADMIN_PROXIED, PROXY_CONNECT_ARG,
    PROXY_CONNECT_LAN, PROXY_CONNECT_LOCALHOST, PROXY_CONNECT_UNLISTED, TARGET_CONNECTED,
    TARGET_REACHED, TARGET_TAKEN,
};
use super::{Bait, CHECK_FILE, Expect, Probe, settings_write, tilde};

/// The probes for `sandbox`; `unlisted`, a host its proxy's list of
/// hosts leaves out, when it has one.
pub fn probes(sandbox: &Confinement, bait: &Bait, os: Os, unlisted: Option<&str>) -> Vec<Probe> {
    let linux = os == Os::Linux;
    // Through its proxy it has the internet but not the host network's
    // gaps, and no localhost of this machine's but its own ports.
    let proxied = matches!(sandbox.net, Net::Proxied { .. });
    let network = matches!(sandbox.net, Net::Host { .. });
    let localhost = matches!(
        sandbox.net,
        Net::Host {
            localhost: true,
            ..
        }
    );
    let home = match sandbox.read_home {
        true => Expect::Allowed,
        false => Expect::Blocked,
    };
    let if_allowed = |yes: bool| match yes {
        true => Expect::Allowed,
        false => Expect::Blocked,
    };
    let other_port = match (localhost, linux && network) {
        (true, _) => Expect::Allowed,
        (false, true) => Expect::Gap("on Linux, network opens every localhost port"),
        (false, false) => Expect::Blocked,
    };
    let probe = |what: &str, script, expect| Probe {
        what: what.into(),
        script,
        expect,
        lands: None,
        arg: None,
        path: None,
        required: false,
    };
    let mut all = vec![
        probe(
            "read a file in your home folder",
            r#"[ "$(cat "$LNK_PROBE_CANARY")" = "$LNK_PROBE_SECRET" ]"#,
            home,
        ),
        probe(
            "list your home folder",
            r#"ls -a "$LNK_PROBE_HOME" | grep -qxF "$(basename "$LNK_PROBE_CANARY")""#,
            home,
        ),
    ];
    if bait.home.join(".ssh").is_dir() {
        all.push(probe(
            "list your SSH keys (~/.ssh)",
            r#"ls "$LNK_PROBE_HOME/.ssh" >/dev/null"#,
            home,
        ));
    }
    all.push(probe(
        "read Link's settings (keys, tokens)",
        r#"[ "$(cat "$LNK_PROBE_SETTINGS")" = "$LNK_PROBE_SECRET" ]"#,
        Expect::Blocked,
    ));
    all.push(Probe {
        lands: Some(settings_write(&bait.settings)),
        ..probe(
            "write in Link's settings (tools, rules)",
            r#"echo x > "$LNK_PROBE_TARGET""#,
            Expect::Blocked,
        )
    });
    for (folder, file) in &bait.hidden {
        all.push(Probe {
            path: Some(file.clone()),
            ..probe(
                &format!("read what's hidden: {}", tilde(folder, &bait.home)),
                r#"[ "$(cat "$LNK_PROBE_PATH")" = "$LNK_PROBE_SECRET" ]"#,
                Expect::Blocked,
            )
        });
    }
    for (folder, file, read) in &bait.reads {
        let (what, expect) = match read {
            true => ("read", Expect::Allowed),
            false => ("read what isn't its own", Expect::Blocked),
        };
        all.push(Probe {
            path: Some(file.clone()),
            required: *read,
            ..probe(
                &format!("{what}: {}", tilde(folder, &bait.home)),
                r#"[ "$(cat "$LNK_PROBE_PATH")" = "$LNK_PROBE_SECRET" ]"#,
                expect,
            )
        });
    }
    // The sandbox runs this program, but nothing beside it: the folder
    // (`~/.lnk/exec/bin`) holds whatever else the user keeps there.
    if let Some(dir) = bait.sandbox.parent().filter(|d| d.starts_with(&bait.home)) {
        all.push(probe(
            &format!(
                "list the folder lnk-sandbox is in ({})",
                tilde(dir, &bait.home)
            ),
            r#"ls -A "$(dirname "$LNK_PROBE_SANDBOX")" | grep -qvxF "$(basename "$LNK_PROBE_SANDBOX")""#,
            home,
        ));
    }
    all.push(probe(
        "read a file outside your home folder (/var/tmp)",
        r#"[ "$(cat "$LNK_PROBE_ELSEWHERE")" = "$LNK_PROBE_SECRET" ]"#,
        Expect::Blocked,
    ));
    all.push(Probe {
        lands: Some(bait.write.clone()),
        ..probe(
            "write in your home folder",
            r#"echo x > "$LNK_PROBE_TARGET""#,
            Expect::Blocked,
        )
    });
    for dir in &bait.writable {
        all.push(Probe {
            lands: Some(dir.join(CHECK_FILE)),
            ..probe(
                &format!("write {}", tilde(dir, &bait.home)),
                r#"echo x > "$LNK_PROBE_TARGET""#,
                Expect::Allowed,
            )
        });
    }
    all.extend(dot_file_probes(bait, os));
    all.push(probe(
        "connect to another server on localhost",
        r#"exec 3<>"/dev/tcp/127.0.0.1/$LNK_PROBE_PORT""#,
        other_port,
    ));
    let mapped = match (other_port, network) {
        (Expect::Blocked, true) => Expect::Gap(
            "macOS's sandbox doesn't count ::ffff:127.0.0.1 as localhost, so with \
             network every localhost port is reachable this way",
        ),
        (expect, _) => expect,
    };
    all.push(probe(
        "connect to it as ::ffff:127.0.0.1",
        r#"exec 3<>"/dev/tcp/::ffff:127.0.0.1/$LNK_PROBE_PORT""#,
        mapped,
    ));
    if bait.lan.is_some() {
        let expect = match (network, localhost) {
            (false, _) => Expect::Blocked,
            (true, true) => Expect::Allowed,
            (true, false) => Expect::Gap(
                "this machine's own address isn't localhost to the sandbox: \
                 a server listening on every address is reachable with network",
            ),
        };
        all.push(probe(
            "connect to it at this machine's network address",
            r#"exec 3<>"/dev/tcp/$LNK_PROBE_LAN/$LNK_PROBE_PORT""#,
            expect,
        ));
    }
    match proxied {
        true => {
            all.push(internet(bait.internet));
            all.push(probe(
                "ask its proxy for the server on localhost",
                PROXY_CONNECT_LOCALHOST,
                Expect::Blocked,
            ));
            if bait.lan.is_some() {
                all.push(probe(
                    "ask its proxy for this machine's network address",
                    PROXY_CONNECT_LAN,
                    Expect::Blocked,
                ));
            }
            if let Some(host) = unlisted {
                all.push(probe(
                    &format!("ask its proxy for a host not on its list ({host})"),
                    PROXY_CONNECT_UNLISTED,
                    Expect::Blocked,
                ));
                if bait.dns {
                    all.push(probe(
                        &format!("look up a host not on its list ({host}) with its DNS"),
                        r#"getent hosts "$LNK_PROBE_UNLISTED" >/dev/null"#,
                        Expect::Blocked,
                    ));
                    for (kind, qtype, name) in [
                        ("TXT", 16, host.to_string()),
                        ("SRV", 33, format!("_x._tcp.{host}")),
                    ] {
                        all.push(Probe {
                            arg: Some(format!("{qtype} {name}")),
                            ..probe(
                                &format!("ask its DNS for {kind} records of {name}"),
                                DNS_ANSWERED,
                                Expect::Blocked,
                            )
                        });
                    }
                }
            }
            for (what, target) in ipv6_forms(bait) {
                all.push(Probe {
                    arg: Some(target),
                    ..probe(
                        &format!("ask its proxy for {what}"),
                        PROXY_CONNECT_ARG,
                        Expect::Blocked,
                    )
                });
            }
        }
        false => all.push(probe(
            "reach the internet (1.1.1.1)",
            r#"curl -s -m 5 -o /dev/null http://1.1.1.1"#,
            if_allowed(network),
        )),
    }
    for port in &bait.ports {
        // macOS: a model's port isn't reachable, only its inference, on
        // the proxy.
        if bait.models.contains(port) && !linux {
            all.push(Probe {
                arg: Some(port.to_string()),
                ..probe(
                    &format!("connect to the model on port {port} directly"),
                    r#"exec 3<>"/dev/tcp/127.0.0.1/$LNK_PROBE_ARG""#,
                    Expect::Blocked,
                )
            });
            all.push(Probe {
                arg: Some(port.to_string()),
                ..probe(
                    &format!("use port {port}'s admin API through its proxy (a model's delete)"),
                    MODEL_ADMIN_PROXIED,
                    Expect::Blocked,
                )
            });
            continue;
        }
        if bait.models.contains(port) {
            all.push(Probe {
                arg: Some(port.to_string()),
                ..probe(
                    &format!("use port {port}'s admin API (a model's delete)"),
                    MODEL_ADMIN,
                    Expect::Blocked,
                )
            });
            continue;
        }
        all.push(Probe {
            arg: Some(port.to_string()),
            ..probe(
                &format!("connect to port {port} here, which it may reach"),
                r#"exec 3<>"/dev/tcp/127.0.0.1/$LNK_PROBE_ARG""#,
                // Unreached: nothing listening, or Linux with no network.
                Expect::Gap(
                    "whatever serves that port, with its whole API: a local model's \
                     admin endpoints (pull, create, delete) too, which write outside \
                     the sandbox",
                ),
            )
        });
    }
    if !linux {
        for (what, service, expect) in services(sandbox) {
            all.push(Probe {
                arg: Some(service.to_string()),
                ..probe(
                    what,
                    r#""$LNK_PROBE_SANDBOX" sandbox lookup "$LNK_PROBE_ARG""#,
                    expect,
                )
            });
        }
    }
    if linux {
        all.push(probe(
            "read another program's environment",
            r#"grep -qaF "$LNK_PROBE_SECRET" "/proc/$LNK_PROBE_PID/environ""#,
            Expect::Blocked,
        ));
        if bait.python {
            for (what, call) in KERNEL {
                all.push(Probe {
                    what: what.to_string(),
                    script: KERNEL_PROBE,
                    expect: Expect::Blocked,
                    lands: None,
                    arg: Some(call.to_string()),
                    path: None,
                    required: false,
                });
            }
        }
        if bait.abstract_socket.is_some() {
            all.push(probe(
                "connect to an abstract unix socket (as X11's)",
                r#"python3 -c 'import os, socket; socket.socket(socket.AF_UNIX).connect("\0" + os.environ["LNK_PROBE_ABSTRACT"])'"#,
                match network {
                    true => Expect::Gap(
                        "on Linux, network shares this machine's abstract unix sockets \
                         (X11's, some session buses'); a proxy instead of network closes it",
                    ),
                    false => Expect::Blocked,
                },
            ));
        }
    } else {
        all.push(probe(
            "read another program's environment",
            r#"ps -wwE -p "$LNK_PROBE_PID" | grep -qF "$LNK_PROBE_SECRET""#,
            Expect::Blocked,
        ));
        all.push(probe(
            "open an app (Finder, in the background)",
            r#"open -g -a Finder"#,
            Expect::Blocked,
        ));
        if bait.keychain.is_some() {
            all.push(probe(
                "find an item in your Keychain",
                r#"security find-generic-password -s "$LNK_PROBE_KEYCHAIN_ITEM" >/dev/null 2>&1"#,
                Expect::Blocked,
            ));
        }
    }
    // Its descriptor 9 is [`LEFT_OPEN`].
    all.push(probe(
        "write a pipe that what started it passed on",
        r#"printf x >&9"#,
        match linux {
            true => Expect::Blocked,
            false => Expect::Gap(
                "on macOS, a program keeps the descriptors what started the sandbox left open",
            ),
        },
    ));
    all
}

/// The probe that `target` (`host:port`) isn't reached: a name with no
/// address here (a machine without DNS, behind an HTTPS proxy alone) is
/// reached only by connecting to it.
/// Reaching the internet (1.1.1.1) through its proxy, by the proxy's own
/// answer to a `CONNECT`, which no site's 403 can be mistaken for. Taken
/// when it may (still dialing counts: nothing answers here on some
/// machines); refused only by its answer, however long a busy machine
/// takes to give it, never by a silence.
pub(super) fn internet(allowed: bool) -> Probe {
    let (script, expect) = match allowed {
        true => (TARGET_TAKEN, Expect::Allowed),
        false => (TARGET_REACHED, Expect::Blocked),
    };
    Probe {
        what: "reach the internet (1.1.1.1), through its proxy".into(),
        script,
        expect,
        lands: None,
        arg: Some("1.1.1.1:443".into()),
        path: None,
        required: false,
    }
}

pub(super) fn refused(target: &str) -> Probe {
    let resolves = link_sandbox_proxy::http::host_port(target, Some(443))
        .is_some_and(|(host, _)| link_sandbox_route::resolve::resolves(&host));
    let (what, script) = match resolves {
        true => (format!("reach {target}"), TARGET_REACHED),
        false => (
            format!("reach {target} (no address here)"),
            TARGET_CONNECTED,
        ),
    };
    Probe {
        what,
        script,
        expect: Expect::Blocked,
        lands: None,
        arg: Some(target.to_string()),
        path: None,
        required: false,
    }
}

/// What no sandbox reaches, written as IPv6 that holds an IPv4 address
/// (mapped, NAT64 and 6to4), each judged as that IPv4 address: this
/// machine (the bait's server), a cloud's metadata service, and a private
/// address its `[lan]` doesn't list. Each as (what, which form, the
/// `CONNECT` target).
pub fn ipv6_forms(bait: &Bait) -> Vec<(String, String)> {
    let private = std::net::Ipv4Addr::new(10, 255, 255, 1);
    let mut targets = vec![
        ("this machine", std::net::Ipv4Addr::LOCALHOST, bait.port),
        (
            "a cloud's metadata service",
            std::net::Ipv4Addr::new(169, 254, 169, 254),
            80,
        ),
    ];
    // A private address its `[lan]` lists is its to reach, however written.
    if !link_sandbox_proxy::lan::allowed(private.into(), &private.to_string(), &bait.lan_hosts) {
        targets.push(("a private address", private, 80));
    }
    let mut forms = Vec::new();
    for (what, ip, port) in targets {
        let [a, b, c, d] = ip.octets();
        let (hi, lo) = (u16::from_be_bytes([a, b]), u16::from_be_bytes([c, d]));
        for (form, v6) in [
            ("mapped", ip.to_ipv6_mapped()),
            ("compatible", ip.to_ipv6_compatible()),
            (
                "NAT64",
                std::net::Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, hi, lo),
            ),
            (
                "6to4",
                std::net::Ipv6Addr::new(0x2002, hi, lo, 0, 0, 0, 0, 1),
            ),
        ] {
            forms.push((
                format!("{what} ({ip}) written as {form} IPv6, {v6}"),
                format!("[{v6}]:{port}"),
            ));
        }
    }
    forms
}

/// The macOS services a probe looks up by name, and what it should find:
/// those every sandbox gets, those its confinement opens, and those no
/// sandbox may reach (each a way to act outside it). Behind a proxy that
/// reaches the internet, only the certificate checks.
pub(super) fn services(sandbox: &Confinement) -> Vec<(&'static str, &'static str, Expect)> {
    let if_allowed = |yes: bool| match yes {
        true => Expect::Allowed,
        false => Expect::Blocked,
    };
    let (network, certificates) = match &sandbox.net {
        Net::Host { .. } => (true, true),
        Net::Proxied { proxy, .. } => (false, !proxy.no_internet),
        _ => (false, false),
    };
    let settings = if_allowed(network);
    vec![
        (
            "reach macOS's users and groups (opendirectoryd)",
            "com.apple.system.opendirectoryd.libinfo",
            Expect::Allowed,
        ),
        (
            "reach macOS's log (logd)",
            "com.apple.logd",
            Expect::Allowed,
        ),
        (
            "reach macOS's notifications (notifyd)",
            "com.apple.system.notification_center",
            Expect::Allowed,
        ),
        (
            "reach the network's settings (configd)",
            "com.apple.SystemConfiguration.configd",
            settings,
        ),
        (
            "reach certificate checks (trustd)",
            "com.apple.trustd",
            if_allowed(certificates),
        ),
        (
            "reach Xcode's preferences (cfprefsd)",
            "com.apple.cfprefsd.daemon",
            if_allowed(sandbox.developer_tools),
        ),
        (
            "reach the Keychain (securityd)",
            "com.apple.SecurityServer",
            Expect::Blocked,
        ),
        (
            "reach LaunchServices (opening apps)",
            "com.apple.coreservices.launchservicesd",
            Expect::Blocked,
        ),
        (
            "reach the clipboard (pboard)",
            "com.apple.pasteboard.1",
            Expect::Blocked,
        ),
        (
            "reach the screen (WindowServer)",
            "com.apple.windowserver.active",
            Expect::Blocked,
        ),
        (
            "send Apple Events to other apps",
            "com.apple.coreservices.appleevents",
            Expect::Blocked,
        ),
        (
            "reach the privacy database (tccd)",
            "com.apple.tccd",
            Expect::Blocked,
        ),
        (
            "reach core services (coreservicesd)",
            "com.apple.CoreServices.coreservicesd",
            Expect::Blocked,
        ),
    ]
}

/// The system calls the seccomp filter refuses that a program can make
/// unprivileged, each tried from inside: what the probe says, and the
/// call's name for [`KERNEL_PROBE`].
const KERNEL: &[(&str, &str)] = &[
    ("make a user namespace (the kernel's inside)", "unshare"),
    ("set up io_uring", "io_uring_setup"),
    ("use userfaultfd", "userfaultfd"),
    ("use the kernel's key store (keyctl)", "keyctl"),
    ("trace a process (ptrace)", "ptrace"),
];
