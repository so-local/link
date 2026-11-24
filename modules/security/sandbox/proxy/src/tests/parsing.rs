//! What the proxy reads: socket paths, addresses in URLs, targets, request
//! heads and a web page's requests told apart.

use super::*;

/// A socket's path past its limit is said with what to do.
#[test]
fn a_socket_path_too_long_says_so() {
    let long = std::path::PathBuf::from(format!("/tmp/{}/in.sock", "x".repeat(120)));
    let err = listen_unix(&long, Default::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("a home with a shorter path"), "{err}");
}

#[test]
fn an_address_in_a_url_is_read_as_a_browser_reads_it() {
    use {ends_in_number, url_ipv4};
    for (host, ip) in [
        ("127.0.0.1", "127.0.0.1"),
        ("127.1", "127.0.0.1"),
        ("127.0.1", "127.0.0.1"),
        ("2130706433", "127.0.0.1"),
        ("0x7f000001", "127.0.0.1"),
        ("0X7F.1", "127.0.0.1"),
        ("0177.0.0.1", "127.0.0.1"),
        ("127.0.0.1.", "127.0.0.1"),
        ("0", "0.0.0.0"),
        ("0x", "0.0.0.0"),
        ("00", "0.0.0.0"),
        ("3232235777", "192.168.1.1"),
        ("192.168.257", "192.168.1.1"),
        ("4294967295", "255.255.255.255"),
    ] {
        assert!(ends_in_number(host), "{host}");
        assert_eq!(url_ipv4(host), Some(ip.parse().unwrap()), "{host}");
    }
    // Ends in a number, but no address: a browser refuses the URL.
    for host in [
        "4294967296",
        "1.2.3.4.5",
        "256.1",
        "1.16777216",
        "08",
        "0x1g",
        "example.09",
        "1..1",
        "127.0.0.1..",
    ] {
        assert_eq!(url_ipv4(host), None, "{host}");
    }
    for name in [
        "example.com",
        "localhost",
        "1.example",
        "0x.com",
        "a.0xg",
        "a..",
    ] {
        assert!(!ends_in_number(name), "{name}");
    }
    // The resolver reads the same forms as the same addresses.
    for host in ["127.1", "2130706433", "0x7f000001", "0177.0.0.1"] {
        let resolved = std::net::ToSocketAddrs::to_socket_addrs(&(host, 1)).map(|mut a| a.next());
        if let Ok(Some(addr)) = resolved {
            assert_eq!(Some(addr.ip()), url_ipv4(host).map(Into::into), "{host}");
        }
    }
}

#[test]
fn targets_parse() {
    assert_eq!(
        host_port("example.com:443", None),
        Some(("example.com".into(), 443))
    );
    assert_eq!(
        host_port("example.com", Some(80)),
        Some(("example.com".into(), 80))
    );
    assert_eq!(host_port("[::1]:8080", None), Some(("::1".into(), 8080)));
    assert_eq!(host_port("example.com", None), None);
    assert_eq!(host_port(":443", None), None);
}

#[tokio::test]
async fn a_request_head_is_bounded_and_whole() {
    let head = |bytes: Vec<u8>| async move { read_head(&mut bytes.as_slice()).await };
    // What follows the head is kept for the request, never parsed as it.
    let (got, rest) = head(b"GET / HTTP/1.1\r\n\r\nGET /again HTTP/1.1\r\n\r\n".to_vec())
        .await
        .unwrap();
    assert_eq!(got, b"GET / HTTP/1.1\r\n\r\n");
    assert_eq!(rest, b"GET /again HTTP/1.1\r\n\r\n");
    // A head without an end, too long, or cut off, is refused.
    let endless = b"GET / HTTP/1.1\r\nX: "
        .iter()
        .chain(&[b'a'; MAX_HEAD + 8192]);
    let err = head(endless.copied().collect()).await.unwrap_err();
    assert!(err.to_string().contains("too large"), "{err}");
    let err = head(b"CONNECT a:443 HTTP/1.1\r\n".to_vec())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("closed"), "{err}");
    assert!(head(b"\n\n\r\n".to_vec()).await.is_err());
}

/// The proxy's verbs are refused to what a web page can send: a name
/// it pointed here (DNS rebinding), or a request from another site.
#[test]
fn a_web_pages_request_is_told_apart() {
    let head = |headers: &str| format!("POST /_link/secret/x/y HTTP/1.1\r\n{headers}\r\n");
    for fine in [
        "",
        "Host: 127.0.0.1:40781\r\n",
        "Host: localhost:20199\r\n",
        "Host: [::1]:20199\r\n",
        "Host: 127.0.0.1\r\nsec-fetch-mode: cors\r\n",
        "Host: 127.0.0.1\r\nOrigin: http://localhost:3000\r\n",
        "Host: 127.0.0.1\r\nSec-Fetch-Site: same-origin\r\n",
    ] {
        let head = head(fine);
        let head = RequestHead::parse(head.as_bytes()).unwrap();
        assert_eq!(from_a_browser(&head), None, "{fine}");
    }
    for browser in [
        "Host: rebound.example:40781\r\n",
        "Host: 127.0.0.1.nip.io:40781\r\n",
        "Host: 127.0.0.1\r\nOrigin: https://evil.example\r\n",
        "Host: 127.0.0.1\r\nOrigin: null\r\n",
        "Host: 127.0.0.1\r\nSec-Fetch-Site: cross-site\r\n",
        "Host: 127.0.0.1\r\nsec-fetch-site: same-site\r\n",
    ] {
        let head = head(browser);
        let head = RequestHead::parse(head.as_bytes()).unwrap();
        assert!(from_a_browser(&head).is_some(), "{browser}");
    }
}

/// Every reader takes a head one way: one space between the request
/// line's parts, HTTP/1.x, each header with a colon and on one line.
#[test]
fn a_head_is_read_one_way_or_refused() {
    let head = b"POST /_link/mcp/gh?x=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 3\r\nExpect: 100-continue\r\n\r\n";
    let request = RequestHead::parse(head).unwrap();
    assert_eq!(
        (request.method, request.target, request.version),
        ("POST", "/_link/mcp/gh?x=1", "HTTP/1.1")
    );
    assert_eq!(request.path(), "/_link/mcp/gh");
    assert_eq!(request.header("content-length"), Some("3"));
    assert_eq!(request.length(), Ok(3));
    assert!(request.expects_continue());
    for (bad, why) in [
        (&b"GET  / HTTP/1.1\r\n\r\n"[..], "not an HTTP request"),
        (b"GET / HTTP/2\r\n\r\n", "isn't HTTP/1.x"),
        (b"GET / HTTP/1.1 x\r\n\r\n", "not an HTTP request"),
        (b"GET / HTTP/1.1\r\nHost: a\r\n folded\r\n\r\n", "folded"),
        (b"GET / HTTP/1.1\r\nno colon\r\n\r\n", "without a colon"),
        (b"GET / HTTP/1.1\nHost: a\r\n\r\n", "bare line end"),
        // A name another reader takes another way: one with a space
        // before its colon, or in it, isn't a header's name.
        (
            b"POST / HTTP/1.1\r\nContent-Length : 5\r\n\r\n",
            "header's name",
        ),
        (
            b"POST / HTTP/1.1\r\nContent Length: 5\r\n\r\n",
            "header's name",
        ),
        // A length of digits alone: `+5` is 5 to some readers, none to others.
        (
            b"POST / HTTP/1.1\r\nContent-Length: +5\r\n\r\n",
            "content-length",
        ),
        (
            b"POST / HTTP/1.1\r\nContent-Length: 5 5\r\n\r\n",
            "content-length",
        ),
    ] {
        let err = RequestHead::parse(bad).unwrap_err();
        assert!(err.contains(why), "{err}");
    }
    let lengths = |h: &str| {
        RequestHead::parse(format!("POST / HTTP/1.1\r\n{h}\r\n").as_bytes())
            .unwrap()
            .length()
    };
    assert_eq!(lengths(""), Ok(0));
    assert_eq!(lengths("Content-Length: 2\r\nContent-Length: 2\r\n"), Ok(2));
    assert!(lengths("Content-Length: 2\r\nContent-Length: 3\r\n").is_err());
    assert!(lengths("Transfer-Encoding: chunked\r\n").is_err());
    assert!(RequestHead::parse(b"POST / HTTP/1.1\r\nContent-Length: x\r\n\r\n").is_err());

    let answer = AnswerHead::parse(b"HTTP/1.1 101 Switching\r\nUpgrade: x\r\n\r\n").unwrap();
    assert!(answer.informational() && answer.code == 101);
    assert_eq!(answer.header("upgrade"), Some("x"));
    assert!(AnswerHead::parse(b"SSH-2.0\r\n\r\n").is_err());
}
