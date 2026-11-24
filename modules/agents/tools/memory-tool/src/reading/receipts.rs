use super::*;

fn read(source: &[u8], from: usize, lines: Option<usize>, tokens: Option<usize>) -> Document {
    slice(
        "skills/example/SKILL.md",
        Kind::File,
        "Example".into(),
        source,
        from,
        lines,
        tokens,
    )
}

#[test]
fn requested_line_count_does_not_establish_whole_source() {
    let source = b"one\r\ntwo\r\nthree\r\n";
    let page = read(source, 2, Some(1), None);
    assert_eq!(page.text, "two");
    assert_eq!(page.next, None);
    assert_eq!(page.read.span.start, 5);
    assert_eq!(page.read.span.end, 10);
    page.read.validate(&page.text).unwrap();
    assert!(!page.read.covers_source());
    let whole = read(source, 1, None, None);
    assert_eq!(whole.text, "one\ntwo\nthree");
    whole.read.validate(&whole.text).unwrap();
    assert!(whole.read.covers_source());
}

#[test]
fn clipping_and_changed_sources_keep_distinct_identity() {
    let source = "é".repeat(20);
    let page = read(source.as_bytes(), 1, None, Some(1));
    assert!(page.cut);
    assert_eq!(page.text, "éééé");
    assert_eq!(page.read.span.end, 8);
    page.read.validate(&page.text).unwrap();
    assert!(!page.read.covers_source());
    let changed = read(b"changed", 1, None, None);
    assert_ne!(page.read.source, changed.read.source);
}

#[test]
fn lossy_line_normalization_never_establishes_full_content() {
    let source = b"a\xffb\r\nc\r\n";
    let page = read(source, 1, None, None);
    assert_eq!(page.text, "a\u{fffd}b\nc");
    page.read.validate(&page.text).unwrap();
    assert_eq!(page.read.span.end, source.len() as u64);
    assert!(!page.read.covers_source());
    assert!(
        page.read
            .transformations
            .contains(&link_files::reading::TextTransform::Lines)
    );
    assert!(
        page.read
            .transformations
            .contains(&link_files::reading::TextTransform::LossyUtf8)
    );
    let clipped = read(b"a\xffbcdefgh", 1, None, Some(1));
    assert_eq!(clipped.text, "a\u{fffd}bc");
    assert_eq!(clipped.read.span.end, 4);
    clipped.read.validate(&clipped.text).unwrap();
    assert!(!clipped.read.covers_source());
}
