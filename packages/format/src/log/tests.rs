use super::*;
use std::time::{Duration, UNIX_EPOCH};

fn line(level: Level, text: &str, ids: Ids) -> Line {
    Line {
        at: Stamp(UNIX_EPOCH + Duration::from_millis(1_792_764_311_042)),
        ..Line::now(level, text, ids)
    }
}

#[test]
fn a_line_is_one_json_record_with_its_time_ids_and_schema() {
    let ids = Ids::agent("3e75d6ac33d355c208c3e1140708b8a9")
        .chat("link:3e75d6ac33d355c208c3e1140708b8a9")
        .call("toolu_01ABCDEFGH");
    let mut written = Vec::new();
    line(Level::Warning, "tool agent__send failed", ids.clone())
        .write(&mut written)
        .unwrap();
    let text = String::from_utf8(written).unwrap();
    assert_eq!(text.matches('\n').count(), 1, "{text}");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["at"], "2026-10-23T14:05:11.042Z");
    assert_eq!(json["level"], "warning");
    assert_eq!(json["agent"], "3e75d6ac33d355c208c3e1140708b8a9");
    assert_eq!(json["chat"], "link:3e75d6ac33d355c208c3e1140708b8a9");
    assert_eq!(json["call"], "toolu_01ABCDEFGH");
    assert_eq!(json["schema"], 1);
    assert!(json.get("turn").is_none(), "{text}");
    let read = Line::parse(&text).unwrap();
    assert_eq!(read.ids(), ids);
    assert_eq!(
        read.at.0,
        UNIX_EPOCH + Duration::from_millis(1_792_764_311_042)
    );
}

#[test]
fn a_line_not_links_is_the_harness_own_cleaned() {
    assert_eq!(
        Entry::read("Listening\x1b[0m\ton 8080"),
        Entry::Said {
            text: "Listening[0m    on 8080".into()
        }
    );
    // A record of a newer schema, or with a field it doesn't know, isn't
    // Link's record as this reader knows it.
    assert!(matches!(
        Entry::read(r#"{"at":"2026-10-23T14:05:11Z","level":"info","text":"x","schema":99}"#),
        Entry::Said { .. }
    ));
}

#[test]
fn the_same_line_in_a_row_shows_once_with_how_many() {
    let warned = || Entry::Line(line(Level::Warning, "tool web left out", Ids::agent("a")));
    let mut other = line(
        Level::Warning,
        "tool web left out",
        Ids::agent("a").chat("link:1"),
    );
    other.at = Stamp(UNIX_EPOCH);
    let shown = collapse([warned(), warned(), warned(), Entry::Line(other), warned()]);
    assert_eq!(shown.iter().map(|s| s.times).collect::<Vec<_>>(), [3, 1, 1]);
}

#[test]
fn a_shown_line_is_its_time_mark_words_and_short_ids() {
    let ids = Ids::agent("3e75d6ac33d355c208c3e1140708b8a9")
        .chat("link:3e75d6ac33d355c208c3e1140708b8a9")
        .call("toolu_01ABCDEFGH");
    let shown = Shown {
        entry: Entry::Line(line(Level::Warning, "tool failed:\nbadly", ids)),
        times: 3,
    };
    let ctx = Ctx::plain();
    let own = shown.render(Some("3e75d6ac33d355c208c3e1140708b8a9"), &ctx);
    let clock = crate::time::local_clock(shown_at(&shown), ctx.now);
    assert_eq!(
        own,
        format!(
            "{clock} ! tool failed: badly  chat link:3e75d6ac\u{2026} \u{b7} call toolu_01\u{2026}  \u{d7}3"
        )
    );
    let other = shown.render(None, &ctx);
    assert!(
        other.contains("agent 3e75d6ac\u{2026} \u{b7} chat"),
        "{other}"
    );
    assert!(!own.contains('{'), "{own}");
}

fn shown_at(shown: &Shown) -> SystemTime {
    match &shown.entry {
        Entry::Line(line) => line.at.0,
        Entry::Said { .. } => unreachable!(),
    }
}
