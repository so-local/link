use super::*;
use link_sandbox_contract::net::Upstream;

fn rules(hosts: &[&str], tcp: &[&str], internet: bool) -> Filter {
    Filter::new(Upstream::Direct)
        .only(hosts.iter().map(|h| h.to_string()).collect())
        .with_tcp(tcp.iter().map(|h| h.to_string()).collect())
        .with_lan(Vec::new(), internet)
}

#[test]
fn a_query_reads_back_and_its_answer_holds_its_question() {
    let asked = query(0x1234, "Shard-00.ABC.mongodb.net", A);
    let q = parse_query(&asked).unwrap();
    assert_eq!((q.id, q.qtype, q.rd), (0x1234, A, true));
    assert_eq!(q.name, "shard-00.abc.mongodb.net");
    let out = answer(
        &q,
        NOERROR,
        &[(Record::A(Ipv4Addr::new(127, 1, 0, 1)), 999)],
        true,
    );
    // Its question as it came, case and all.
    assert_eq!(&out[12..12 + q.question.len()], &q.question[..]);
    let (rcode, records) = read_answer(&out, 0x1234, &q.name, A).unwrap();
    assert_eq!(rcode, NOERROR);
    assert_eq!(records, [(Record::A(Ipv4Addr::new(127, 1, 0, 1)), MAX_TTL)]);
}

#[test]
fn what_isnt_a_plain_query_is_refused() {
    assert_eq!(parse_query(&[0; 5]), Err(None));
    let mut answer_bit = query(1, "a.com", A);
    answer_bit[2] |= 0x80;
    assert_eq!(parse_query(&answer_bit), Err(None));
    let mut two = query(2, "a.com", A);
    two[5] = 2;
    assert_eq!(parse_query(&two), Err(Some((2, FORMERR))));
    // A pointer in the question, a name of another alphabet.
    let mut pointer = query(3, "a.com", A);
    pointer.truncate(12);
    pointer.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1]);
    assert_eq!(parse_query(&pointer), Err(Some((3, FORMERR))));
    let mut odd = query(4, "a.com", A);
    odd[13] = b'*';
    assert_eq!(parse_query(&odd), Err(Some((4, REFUSED))));
}

#[test]
fn only_the_names_its_rules_reach_are_answered() {
    let db = rules(&["*.abc.mongodb.net"], &["*.abc.mongodb.net:27017"], true);
    assert_eq!(plan("shard-00.abc.mongodb.net", A, &db), Plan::Own);
    assert_eq!(plan("shard-00.abc.mongodb.net", AAAA, &db), Plan::Own);
    assert_eq!(
        plan("_mongodb._tcp.cluster0.abc.mongodb.net", SRV, &db),
        Plan::Ask
    );
    assert_eq!(plan("cluster0.abc.mongodb.net", TXT, &db), Plan::Ask);
    // A service of the domain itself, which the wildcard leaves out.
    assert!(matches!(
        plan("_x._tcp.abc.mongodb.net", SRV, &db),
        Plan::Refuse(_)
    ));
    for refused in [
        ("abc.mongodb.net", A),
        ("x.other.mongodb.net", A),
        ("_mongodb._tcp.other.mongodb.net", SRV),
        ("example.com", TXT),
    ] {
        assert!(
            matches!(plan(refused.0, refused.1, &db), Plan::Refuse(_)),
            "{refused:?}"
        );
    }
    // A listed host, not an endpoint: its public addresses.
    let both = rules(
        &["github.com", "db.example.com"],
        &["db.example.com:5432"],
        true,
    );
    assert_eq!(plan("github.com", A, &both), Plan::Public);
    assert_eq!(plan("github.com", 65, &both), Plan::Empty);
    assert_eq!(plan("db.example.com", A, &both), Plan::Own);
    // An endpoint its hosts don't list, or no internet: refused.
    let unlisted = rules(&["github.com"], &["db.example.com:5432"], true);
    assert!(matches!(
        plan("db.example.com", A, &unlisted),
        Plan::Refuse(_)
    ));
    let lan = rules(&[], &["db.example.com:5432"], false);
    assert!(matches!(plan("db.example.com", A, &lan), Plan::Refuse(_)));
    // Any host: any name.
    let any = rules(&[], &["db.example.com:5432"], true);
    assert_eq!(plan("anything.example.org", SRV, &any), Plan::Ask);
    // With an HTTP upstream, nothing is asked of this machine's resolver:
    // only the endpoints' own names are answered.
    let upstream: SocketAddr = "127.0.0.1:3128".parse().unwrap();
    let http = Filter {
        upstream: Box::leak(Box::new(Upstream::Http(upstream))),
        ..both
    };
    assert_eq!(plan("github.com", A, &http), Plan::Empty);
    assert_eq!(plan("github.com", TXT, &http), Plan::Empty);
    assert_eq!(plan("db.example.com", A, &http), Plan::Own);
    assert!(matches!(
        plan("other.example.org", A, &http),
        Plan::Refuse(_)
    ));
}

#[test]
fn a_local_networks_service_records_are_refused() {
    let any = Filter::new(Upstream::Direct)
        .with_tcp(vec!["db.example.com:5432".into()])
        .with_lan(Vec::new(), true);
    for local in [
        "_ldap._tcp.dc._msdcs.corp",
        "printer._ipp._tcp.local",
        "_http._tcp.box.home.arpa",
        "nas.lan",
        "_x._tcp.intranet",
        "_x._tcp.db.test",
        "_x._tcp.example",
        "_x._tcp.a.invalid",
        "_x._tcp.svc.alt",
        "_x._tcp.abc.onion",
        "router",
        "1.0.0.10.in-addr.arpa",
    ] {
        for qtype in [SRV, TXT] {
            assert!(
                matches!(plan(local, qtype, &any), Plan::Refuse(_)),
                "{local}"
            );
        }
    }
    assert_eq!(
        plan("_mongodb._tcp.c.abc.mongodb.net", SRV, &any),
        Plan::Ask
    );
}

#[test]
fn each_name_keeps_one_address_of_the_sandboxs_own() {
    let mut names = Names::with(&[("db.example.com".into(), 5432, Ipv4Addr::new(127, 0, 1, 1))]);
    assert_eq!(
        names.address("db.example.com"),
        Some(Ipv4Addr::new(127, 0, 1, 1))
    );
    let a = names.address("a.abc.mongodb.net").unwrap();
    let b = names.address("b.abc.mongodb.net").unwrap();
    assert_eq!(
        (a, b),
        (Ipv4Addr::new(127, 1, 0, 1), Ipv4Addr::new(127, 1, 0, 2))
    );
    assert_eq!(names.address("a.abc.mongodb.net"), Some(a));
    assert_eq!(names.name(b).as_deref(), Some("b.abc.mongodb.net"));
    assert_eq!(names.name(Ipv4Addr::new(127, 1, 0, 9)), None);
}

#[test]
fn an_answer_gives_only_the_records_asked_for() {
    // An SRV answer through an alias, with an A record of another name
    // beside it, which is dropped.
    let mut msg = query(7, "_mongodb._tcp.c.abc.mongodb.net", SRV);
    msg[2] |= 0x80;
    msg[7] = 3;
    let rr = |msg: &mut Vec<u8>, owner: &str, rtype: u16, data: &[u8]| {
        write_name(msg, owner);
        msg.extend_from_slice(&rtype.to_be_bytes());
        msg.extend_from_slice(&[0, 1, 0, 0, 0, 60]);
        msg.extend_from_slice(&(data.len() as u16).to_be_bytes());
        msg.extend_from_slice(data);
    };
    let mut alias = Vec::new();
    write_name(&mut alias, "_mongodb._tcp.real.abc.mongodb.net");
    rr(&mut msg, "_mongodb._tcp.c.abc.mongodb.net", CNAME, &alias);
    let mut srv = vec![0, 0, 0, 0, 0x69, 0x89];
    write_name(&mut srv, "shard-00.abc.mongodb.net");
    rr(&mut msg, "_mongodb._tcp.real.abc.mongodb.net", SRV, &srv);
    rr(&mut msg, "evil.example.com", A, &[1, 2, 3, 4]);
    let (rcode, records) = read_answer(&msg, 7, "_mongodb._tcp.c.abc.mongodb.net", SRV).unwrap();
    assert_eq!(rcode, NOERROR);
    assert_eq!(
        records,
        [(
            Record::Srv {
                priority: 0,
                weight: 0,
                port: 27017,
                target: "shard-00.abc.mongodb.net".into()
            },
            60
        )]
    );
    // Another id, or another question: not this answer.
    assert!(read_answer(&msg, 8, "_mongodb._tcp.c.abc.mongodb.net", SRV).is_err());
    assert!(read_answer(&msg, 7, "other.com", SRV).is_err());
}

#[test]
fn a_long_udp_answer_is_cut_for_tcp() {
    let q = parse_query(&query(9, "c.abc.mongodb.net", TXT)).unwrap();
    let long = vec![(Record::Txt(vec![vec![b'x'; 200]; 4]), 60)];
    let udp = answer(&q, NOERROR, &long, true);
    assert!(udp[2] & 0x02 != 0 && udp[7] == 0);
    let tcp = answer(&q, NOERROR, &long, false);
    assert_eq!(
        read_answer(&tcp, 9, "c.abc.mongodb.net", TXT).unwrap().1,
        long
    );
}

#[test]
fn nameservers_come_from_resolv_conf() {
    let text = "# x\nnameserver 8.8.8.8\nnameserver fe80::1%eth0\nsearch lan\nnameserver ::1\nnameserver 1.1.1.1\n";
    let servers = nameservers_in(text);
    assert_eq!(servers.len(), 3);
    assert_eq!(servers[0], "8.8.8.8:53".parse().unwrap());
}

proptest::proptest! {
    #[test]
    fn no_message_breaks_the_reader(msg in proptest::collection::vec(proptest::num::u8::ANY, 0..300)) {
        let _ = parse_query(&msg);
        let _ = read_answer(&msg, 1, "a.com", SRV);
    }
}
