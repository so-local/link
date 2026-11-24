use super::*;

fn receipt(text: &str) -> ReadReceipt {
    ReadReceipt {
        path: "skills/example/SKILL.md".into(),
        source: ReadSnapshot::Whole {
            content: ContentDigest::of(text.as_bytes()),
        },
        span: ByteSpan {
            start: 0,
            end: text.len() as u64,
        },
        text: ContentDigest::of(text.as_bytes()),
        text_prefix_bytes: 0,
        transformations: vec![],
    }
}

#[test]
fn received_text_and_source_coverage_are_separate() {
    let whole = receipt("one\ntwo\n");
    whole.validate("one\ntwo\n").unwrap();
    assert!(whole.covers_source());
    let mut partial = whole.clone();
    partial.span.start = 4;
    partial.text = ContentDigest::of(b"two\n");
    partial.text_prefix_bytes = 7;
    partial.validate("[page]\ntwo\n").unwrap();
    assert!(!partial.covers_source());
    assert!(partial.validate("[page]\nwrong").is_err());
    let mut prefix = whole.clone();
    prefix.source = ReadSnapshot::Prefix {
        content: whole.source.content().clone(),
        total_bytes: Some(100),
    };
    prefix.validate("one\ntwo\n").unwrap();
    assert!(!prefix.covers_source());
}

#[test]
fn normalization_can_compose_without_crediting_lossy_decoding() {
    let mut read = receipt("é\r\n");
    read.text = ContentDigest::of("é".as_bytes());
    read.transformations = vec![TextTransform::Lines, TextTransform::Trim];
    read.validate("é").unwrap();
    assert!(read.covers_source());
    read.transformations.push(TextTransform::LossyUtf8);
    read.validate("é").unwrap();
    assert!(!read.covers_source());
}

#[test]
fn invalid_ranges_prefixes_digests_and_unknown_fields_are_refused() {
    let good = receipt("é");
    let mut invalid = good.clone();
    invalid.text_prefix_bytes = 1;
    assert!(invalid.validate("é").is_err());
    invalid.text_prefix_bytes = 3;
    assert!(invalid.validate("é").is_err());
    invalid = good.clone();
    invalid.span.end += 1;
    assert!(invalid.validate("é").is_err());
    invalid = good.clone();
    invalid.span.start = 2;
    invalid.span.end = 1;
    assert!(invalid.validate("é").is_err());
    invalid = good.clone();
    invalid.source = ReadSnapshot::Prefix {
        content: good.source.content().clone(),
        total_bytes: Some(0),
    };
    assert!(invalid.validate("é").is_err());
    invalid = good.clone();
    invalid.text.sha256 = "00".into();
    assert!(invalid.validate("é").is_err());
    let mut json = serde_json::to_value(&good).unwrap();
    json["claimed_complete"] = true.into();
    assert!(serde_json::from_value::<ReadReceipt>(json).is_err());
}
