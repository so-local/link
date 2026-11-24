//! A connection's opinions as a spec checks them: activity's modes, a
//! call's line's mark and separator, and who marks a message.

use super::parse;

/// Activity takes its three modes, and a mark or separator is at most
/// eight characters on one line, counted as characters, not bytes.
#[test]
fn a_connections_activity_is_checked() {
    for mode in ["replace", "keep", "off"] {
        parse(&format!("[connection.telegram]\nactivity = \"{mode}\"")).unwrap();
    }
    parse("[connection.slack]\nactivity_mark = \"🔧\"\nactivity_separator = \" → \"").unwrap();
    // Eight `›` are 24 bytes.
    let marked = |n: usize| {
        parse(&format!(
            "[connections]\nactivity_mark = \"{}\"",
            "›".repeat(n)
        ))
    };
    marked(8).unwrap();
    assert!(marked(9).is_err());
    for bad in [
        "[connections]\nactivity = true",
        "[connections]\nactivity = \"log\"",
        "[connections]\nactivity_mark = \"a mark too long\"",
        "[connections]\nactivity_separator = \":\\n\"",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
}

/// Discord marks a message, as Slack does.
#[test]
fn discord_marks_a_message() {
    parse("[connection.discord]\nworking = \"eyes\"\nsteered = \"wave\"").unwrap();
}
