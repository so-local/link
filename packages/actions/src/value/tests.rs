use super::*;

#[test]
fn each_check_names_where_and_why() {
    assert!(pattern("name", "web", "^[a-z0-9-]+$").is_ok());
    let refused = pattern("name", "Web", "^[a-z0-9-]+$").unwrap_err();
    assert_eq!(
        refused.to_string(),
        "name: \"Web\" doesn't match ^[a-z0-9-]+$"
    );
    assert!(length("name", "abcd", None, Some(3)).is_err());
    assert!(items("stages", 0, Some(1), None).is_err());
    assert!(bounds("port", 70_000_i64, Some(1), Some(65_535)).is_err());
    assert!(id("box", "box:k3m4n5p6q7r8", "box").is_ok());
    assert!(id("box", "secret:k3m4n5p6q7r8", "box").is_err());
    assert_eq!(field("", "name"), "name");
    assert_eq!(field("stages[1]", "run"), "stages[1].run");
}

#[test]
fn a_duration_reads_and_prints_as_written() {
    let d: Duration = "30m".parse().unwrap();
    assert_eq!(d.get(), std::time::Duration::from_secs(1800));
    assert_eq!(d.to_string(), "30m");
    for bad in [
        "",
        "30",
        "m",
        "1.5h",
        "-1s",
        "30 m",
        "99999999999999999999d",
    ] {
        assert!(bad.parse::<Duration>().is_err(), "{bad:?} read");
    }
}

#[test]
fn a_duration_from_a_length_takes_its_largest_whole_unit() {
    let of = |secs: u64| Duration::from_std(std::time::Duration::from_secs(secs)).to_string();
    assert_eq!(
        (of(7200), of(90), of(60), of(0)),
        ("2h".into(), "90s".into(), "1m".into(), "0ms".into())
    );
    let half = Duration::from_std(std::time::Duration::from_millis(1500));
    assert_eq!(half.to_string(), "1500ms");
}

#[test]
fn a_size_reads_its_unit_and_writes_its_largest_whole_one() {
    let size: Size = "16 MB".parse().unwrap();
    assert_eq!(size.get(), 16 << 20);
    assert_eq!(
        "16MB".parse::<Size>().unwrap(),
        size,
        "the space is optional"
    );
    assert_eq!(Size::bytes(2048).to_string(), "2 KB");
    assert_eq!(Size::bytes(1500).to_string(), "1500 B");
    for bad in ["", "MB", "16 mb", "1.5 MB", "-1 MB", "99999999999999 GB"] {
        assert!(bad.parse::<Size>().is_err(), "{bad:?} read");
    }
}

#[test]
fn a_rate_reads_how_many_in_each_period() {
    let rate: Rate = "600/m".parse().unwrap();
    assert_eq!((rate.count(), rate.per().as_secs()), (600, 60));
    assert_eq!(rate.to_string(), "600/m");
    for bad in ["", "10", "0/s", "10/d", "x/s", "-1/s"] {
        assert!(bad.parse::<Rate>().is_err(), "{bad:?} read");
    }
}
