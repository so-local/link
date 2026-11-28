use super::*;
mod copying;
#[cfg(target_os = "linux")]
mod ownership;

const TIMEOUT: Duration = Duration::from_secs(120);

#[test]
fn subprocess_exchange_is_bounded_and_refuses_failed_children() {
    let home = link_test_kit::TempDir::new("protocol-exchange");
    let value = serde_json::json!({"a":"typed"});
    let reply: serde_json::Value =
        exchange(link_test_kit::env::command("cat", &home), &value, TIMEOUT).unwrap();
    assert_eq!(reply, value);
    assert!(
        exchange::<_, serde_json::Value>(
            link_test_kit::env::command("false", &home),
            &value,
            TIMEOUT
        )
        .is_err()
    );
    let mut flood = link_test_kit::env::command("head", &home);
    flood.args(["-c", "1048577", "/dev/zero"]);
    assert!(exchange::<_, serde_json::Value>(flood, &value, TIMEOUT).is_err());
    let mut blocked = link_test_kit::env::command("sh", &home);
    blocked.args(["-c", "while :; do :; done"]);
    assert!(exchange::<_, serde_json::Value>(blocked, &value, Duration::ZERO).is_err());
}

/// A child that ends without reading its request is reported by how it
/// exited, never by the pipe it left: a request larger than a pipe holds
/// always meets the closed pipe, and the answer is the same whether or not
/// the child had ended before the first write.
#[test]
fn a_child_that_ends_unread_is_reported_by_its_exit() {
    let home = link_test_kit::TempDir::new("protocol-unread");
    let value = serde_json::json!({ "big": "x".repeat(256 * 1024) });
    let mut refusing = link_test_kit::env::command("sh", &home);
    refusing.args(["-c", "exit 3"]);
    let err = exchange::<_, serde_json::Value>(refusing, &value, TIMEOUT).unwrap_err();
    assert!(
        err.to_string()
            .contains("subprocess exited with exit status: 3"),
        "{err}"
    );
}

#[test]
fn framing_keeps_following_stream_bytes_and_refuses_oversized_headers() {
    let mut input = std::io::Cursor::new(b"{\"version\":1}\nstream bytes");
    let header: serde_json::Value = read_line(&mut input).unwrap();
    assert_eq!(header["version"], 1);
    let mut stream = String::new();
    input.read_to_string(&mut stream).unwrap();
    assert_eq!(stream, "stream bytes");
    assert!(
        read_line::<serde_json::Value>(&mut std::io::Cursor::new(vec![
            b'x';
            MAX_MESSAGE_BYTES + 1
        ]))
        .is_err()
    );
}
