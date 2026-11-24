//! Which hosts are reached: names resolved as given, your network and the
//! internet, endpoints by name, git over ssh, this machine forbidden.

use super::*;

#[test]
fn a_tests_names_are_resolved_as_it_gives_them() {
    let names = "db.example=192.0.2.1, DB.example=2001:db8::1,other.example=192.0.2.2,bad=x";
    let ips = |name| faked_in(names, name);
    assert_eq!(
        ips("db.example"),
        Some(vec![
            "192.0.2.1".parse().unwrap(),
            "2001:db8::1".parse().unwrap()
        ])
    );
    assert_eq!(
        ips("other.example"),
        Some(vec!["192.0.2.2".parse().unwrap()])
    );
    assert_eq!(ips("bad"), None);
    assert_eq!(ips("example"), None);
}

#[test]
fn your_network_is_dialed_from_here_and_the_internet_through_the_upstream() {
    let socks = &Upstream::Socks5("127.0.0.1:1080".parse().unwrap());
    let rules = Filter::new(socks.clone()).with_lan(vec!["192.168.77.0/24".into()], true);
    let (public, lan) = ("1.1.1.1".parse().unwrap(), "192.168.77.5".parse().unwrap());
    // One name with both: each its own way, never the LAN upstream.
    assert_eq!(
        route(&rules, "both.example", true, true, public),
        Some(socks)
    );
    assert_eq!(
        route(&rules, "both.example", true, true, lan),
        Some(&Upstream::Direct)
    );
    // Without [lan] granting it, a private address isn't reached.
    assert_eq!(route(&rules, "both.example", true, false, lan), None);
    let closed = Filter::new(socks.clone());
    assert_eq!(route(&closed, "both.example", true, true, lan), None);
    // No internet: only the network's.
    assert_eq!(route(&rules, "both.example", false, true, public), None);
}

#[test]
fn endpoints_by_name_get_an_address_and_a_line_in_hosts() {
    let tcp = [
        "github.com:22".to_string(),
        "db.example.com:5432".into(),
        "10.1.2.3:5432".into(),
        "db.example.com:6379".into(),
        "cache.example.com:6379".into(),
    ];
    let named = named_endpoints(&tcp);
    // Port 22 is below 1024 and 10.1.2.3 has no name: `sandbox
    // connect` only.
    let got: Vec<_> = named
        .iter()
        .map(|(h, p, a)| (h.as_str(), *p, a.to_string()))
        .collect();
    assert_eq!(
        got,
        [
            ("db.example.com", 5432, "127.0.1.1".into()),
            ("db.example.com", 6379, "127.0.1.2".into()),
            ("cache.example.com", 6379, "127.0.1.3".into()),
        ]
    );
    // One line a name, at its first address, after the machine's own.
    let hosts = etc_hosts("127.0.0.1 localhost\n", &named, &[], true);
    assert_eq!(
        hosts,
        "127.0.0.1 localhost\n# Link: endpoints its proxy reaches (tcp)\n\
         127.0.1.1 db.example.com\n127.0.1.3 cache.example.com\n"
    );
    // A name its hosts refuse, or any with no internet, gets no address.
    let listed = etc_hosts(
        "127.0.0.1 localhost\n",
        &named,
        &["db.example.com".into()],
        true,
    );
    assert!(listed.contains("db.example.com") && !listed.contains("cache"));
    let closed = etc_hosts("127.0.0.1 localhost\n", &named, &[], false);
    assert!(!closed.contains("example.com"), "{closed}");
}

#[test]
fn git_goes_over_ssh_through_the_proxy() {
    assert_eq!(
        git_ssh_command(Path::new("/opt/lnk/lnk-sandbox")).as_deref(),
        Some("ssh -o ProxyCommand='/opt/lnk/lnk-sandbox connect %h %p'")
    );
    // A path a shell would read otherwise: none, rather than a command
    // that runs something else.
    for odd in [
        "/a b/lnk-sandbox",
        "/a'b/lnk-sandbox",
        "/a;b/lnk-sandbox",
        "/$(x)/lnk",
    ] {
        assert_eq!(git_ssh_command(Path::new(odd)), None, "{odd}");
    }
}

#[test]
fn this_machine_and_its_networks_are_forbidden() {
    for ip in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "0.0.0.0",
        "100.64.0.1",
        "::1",
        "fd00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        // IPv4 inside IPv6 by other ways: compatible, NAT64, 6to4.
        "::10.0.0.1",
        "::169.254.169.254",
        "64:ff9b::10.0.0.1",
        "64:ff9b::169.254.169.254",
        "64:ff9b:1::1",
        "2002:a00:1::1",
        "2002:c0a8:101::1",
        // Protocol assignments, benchmarking, reserved.
        "192.0.0.1",
        "198.18.0.1",
        "198.19.255.1",
        "240.0.0.1",
        "255.255.255.254",
        // Azure's host for its machines.
        "168.63.129.16",
        "::ffff:168.63.129.16",
    ] {
        assert!(forbidden(ip.parse().unwrap()), "{ip}");
    }
    for ip in [
        "1.1.1.1",
        "8.8.8.8",
        "2606:4700::1111",
        "::ffff:1.1.1.1",
        "64:ff9b::1.1.1.1",
        "2002:101:101::1",
        "198.20.0.1",
        "192.0.32.1",
    ] {
        assert!(!forbidden(ip.parse().unwrap()), "{ip}");
    }
}

#[test]
fn this_machines_own_addresses_are_refused_whatever_their_range() {
    assert!(this_machine("127.0.0.1".parse().unwrap()));
    // Its address on its network, public or not.
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
    if socket.connect("192.0.2.1:9").is_ok() {
        let own = socket.local_addr().unwrap().ip();
        assert!(this_machine(own), "{own}");
        assert!(!(Filter::new(Upstream::Direct).allowed)(own), "{own}");
    }
    assert!(!this_machine("1.1.1.1".parse().unwrap()));
}

#[tokio::test]
async fn only_the_hosts_listed_are_reached() {
    let target = echo().await;
    let rules = Filter {
        upstream: &Upstream::Direct,
        allowed: |_| true,
        hosts: None,
        tcp: Vec::leak(vec![
            format!("localhost:{}", target.port()),
            target.to_string(),
        ]),
        wire: None,
        routes: &crate::routes::NO_ROUTES,
        calling: None,
        url_secret: None,
        proxy_secret: None,
        lan: &[],
        internet: true,
        lan_only: false,
    }
    .only(vec!["localhost".into()]);
    let answer = through(
        rules,
        &format!("CONNECT localhost:{} HTTP/1.1\r\n\r\n", target.port()),
    )
    .await;
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    // Refused by name, before anything is resolved or dialed.
    let answer = through(rules, "CONNECT example.com:443 HTTP/1.1\r\n\r\n").await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    let answer = through(rules, &format!("CONNECT {target} HTTP/1.1\r\n\r\n")).await;
    assert!(
        answer.starts_with("HTTP/1.1 403"),
        "an address not listed: {answer}"
    );
    let answer = through(rules, "GET http://example.com/ HTTP/1.1\r\n\r\n").await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
}
