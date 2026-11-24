use super::*;

#[test]
fn unknown_evidence_versions_and_fields_are_refused() {
    let proposed = Evidence::new(Record::Proposed(Proposal {
        phase: Phase::Turn,
        attempt: "attempt".into(),
        work: None,
        started_unix_ms: 1,
        calls: vec![],
    }));
    let mut value = serde_json::to_value(&proposed).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(
        serde_json::from_value::<Evidence>(value.clone()).unwrap(),
        proposed
    );
    value["version"] = 2.into();
    assert!(serde_json::from_value::<Evidence>(value.clone()).is_err());
    value["version"] = 1.into();
    value["record"]["credential"] = "never accepted".into();
    assert!(serde_json::from_value::<Evidence>(value).is_err());
}

#[test]
fn execution_identity_is_scoped_to_the_durable_proposal() {
    let first = CallRef {
        proposal: 3,
        ordinal: 0,
    };
    let next = CallRef {
        proposal: 8,
        ordinal: 0,
    };
    assert_ne!(first.portable_id(), next.portable_id());
    assert_eq!(first.portable_id(), "execution-3-0");
}

#[test]
fn omission_is_distinct_from_a_retained_null_or_empty_value() {
    let omitted = Captured::<Value>::Omitted {
        bytes: 30,
        sha256: "digest".into(),
        reason: "capture limit".into(),
    };
    let null = Captured::Retained { value: Value::Null };
    assert_ne!(omitted, null);
    let value = serde_json::to_value(&omitted).unwrap();
    assert_eq!(value["kind"], "omitted");
    assert_eq!(
        serde_json::from_value::<Captured<Value>>(value).unwrap(),
        omitted
    );
}

mod continued {
    use super::*;
    use crate::evidence::index::{Located, index};
    use crate::evidence::request::*;
    use link_files::reading::ContentDigest;

    fn message(text: &str) -> RequestMessage {
        RequestMessage {
            role: "user".into(),
            text: ContentDigest::of(text.as_bytes()),
            answers: vec![],
            calls: vec![],
            media: vec![],
        }
    }

    /// Messages `texts`, the one numbered `seq` of a conversation the
    /// source of each.
    fn projected(texts: &[&str], seqs: &[u64]) -> Projected {
        Projected {
            messages: texts.iter().map(|t| message(t)).collect(),
            sources: seqs
                .iter()
                .zip(texts)
                .enumerate()
                .map(|(i, (seq, text))| Contribution {
                    source: Source::Native { seq: *seq },
                    message: i as u32,
                    slot: Slot::Text {
                        start: 0,
                        end: text.len() as u64,
                    },
                    source_text: ContentDigest::of(text.as_bytes()),
                    projected_text: ContentDigest::of(text.as_bytes()),
                    coverage: Coverage::Whole,
                    media: vec![],
                    omissions: vec![],
                })
                .collect(),
        }
    }

    fn requested(projection: RequestProjection) -> Evidence {
        Evidence::new(Record::ModelRequested {
            request: ModelRequest {
                phase: Phase::Turn,
                attempt: "attempt".into(),
                work: None,
                model: None,
                provider: "local".into(),
                system: ContentDigest::of(b"system"),
                tools: vec![],
                no_calls: false,
                projection: Captured::Retained { value: projection },
                media_delivery: MediaDelivery::CanonicalOnly,
            },
        })
    }

    /// A request that keeps the end of the one before and adds to it is
    /// recorded as what changed, and made again exactly from it.
    #[test]
    fn a_request_is_what_changed_since_the_one_before() {
        let first = projected(&["a", "b", "c"], &[1, 2, 3]);
        // The window dropped "a" and the conversation added "d".
        let next = projected(&["b", "c", "d"], &[2, 3, 5]);
        let continued = first.continue_to(4, &next).unwrap();
        assert_eq!((continued.after, continued.from, continued.kept), (4, 1, 2));
        assert_eq!(continued.messages, vec![message("d")]);
        assert_eq!(continued.sources.len(), 1);
        assert_eq!(continued.sources[0].message, 2);
        assert_eq!(first.continued(&continued).unwrap(), next);
        // One that begins with none of it, a summary first, is whole.
        let summarized = projected(&["summary", "d"], &[6, 5]);
        assert!(first.continue_to(4, &summarized).is_none());
        // Keeping more than there is is refused.
        let mut wrong = continued.clone();
        wrong.kept = 3;
        assert!(first.continued(&wrong).is_err());
    }

    /// The index makes each request whole from the ones before it,
    /// refuses one naming another than the request before it, and leaves
    /// unknown one whose start the records don't hold.
    #[test]
    fn the_index_makes_continued_requests_whole() {
        let first = projected(&["a", "b"], &[1, 2]);
        let next = projected(&["a", "b", "c"], &[1, 2, 4]);
        let last = projected(&["b", "c", "d"], &[2, 4, 6]);
        let records = [
            (3, requested(RequestProjection::Whole(first.clone()))),
            (
                5,
                requested(RequestProjection::Continued(
                    first.continue_to(3, &next).unwrap(),
                )),
            ),
            (
                7,
                requested(RequestProjection::Continued(
                    next.continue_to(5, &last).unwrap(),
                )),
            ),
        ];
        let located = || {
            records.iter().map(|(seq, evidence)| Located {
                seq: *seq,
                evidence,
            })
        };
        let trace = index(located()).unwrap();
        assert_eq!(trace.projection(3).unwrap(), first);
        assert_eq!(trace.projection(5).unwrap(), next);
        assert_eq!(trace.projection(7).unwrap(), last);
        // Without its start, the rest is valid and unknown.
        let trace = index(located().skip(1)).unwrap();
        assert!(trace.projection(7).is_none());
        // Naming a request that isn't the one before is refused.
        let skipping = [
            (3, requested(RequestProjection::Whole(first.clone()))),
            (5, requested(RequestProjection::Whole(next.clone()))),
            (
                7,
                requested(RequestProjection::Continued(
                    next.continue_to(3, &last).unwrap(),
                )),
            ),
        ];
        assert!(
            index(skipping.iter().map(|(seq, evidence)| Located {
                seq: *seq,
                evidence
            }))
            .is_err()
        );
    }
}
