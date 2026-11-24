use super::*;

/// Times compare as instants: across a DST change, Google Cloud's
/// offset changes, and the text sorts them wrong.
#[test]
fn image_times_compare_as_instants() {
    assert_eq!(instant("1970-01-01T00:00:00Z"), Some((0, 0)));
    assert_eq!(
        instant("2026-03-08T01:30:00.000-08:00"),
        instant("2026-03-08T09:30:00Z")
    );
    assert_eq!(
        instant("2024-02-29T12:00:00.5Z"),
        Some((1_709_208_000, 500_000_000))
    );
    // As the clocks go back, 01:10 PST is 20 minutes after 01:50
    // PDT, though its text sorts first.
    let earlier = instant("2026-11-01T01:50:00.000-07:00").unwrap();
    let later = instant("2026-11-01T01:10:00.000-08:00").unwrap();
    assert!(later > earlier);
    assert!("2026-11-01T01:10:00.000-08:00" < "2026-11-01T01:50:00.000-07:00");
    for bad in [
        "",
        "yesterday",
        "2026-13-01T00:00:00Z",
        "2026-01-01T00:00:00",
    ] {
        assert_eq!(instant(bad), None, "{bad}");
    }
}
