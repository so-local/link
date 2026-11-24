use super::*;
use crate::{
    approval::*,
    sources,
    tests::{candidate, ingress},
};

fn shapes() -> Vec<AnswerShape> {
    let choice = |max| AnswerShape::Choice {
        options: vec![
            Choice {
                id: ChoiceId::new("brief").unwrap(),
                label: "Brief".into(),
                description: Some("A concise report".into()),
                recommended: true,
            },
            Choice {
                id: ChoiceId::new("detail").unwrap(),
                label: "Detailed".into(),
                description: None,
                recommended: false,
            },
        ],
        min_selected: 1,
        max_selected: max,
        allow_other: true,
    };
    vec![
        AnswerShape::Confirmation,
        AnswerShape::Text {
            max_bytes: 1000,
            allow_empty: false,
        },
        choice(1),
        choice(2),
    ]
}

#[test]
fn declarations_drive_every_source_domain_shape_surface_and_host_cell() {
    let mut cells = BTreeSet::new();
    let registered: BTreeSet<_> = sources().iter().map(|s| s.id.clone()).collect();
    let domains = render::domains();
    assert_eq!(
        registered,
        domains.iter().flat_map(|d| d.sources.clone()).collect()
    );
    for declared in surfaces() {
        for host in &declared.on {
            assert!(
                cells.insert((declared.id.clone(), *host)),
                "duplicate surface/host registration"
            );
            assert!(
                !declared.inline,
                "an inline adapter needs its own anchored-control chain"
            );
            for domain in domains {
                for source in &domain.sources {
                    for shape in shapes() {
                        let root = link_test_kit::TempDir::new("interaction-matrix");
                        let store = Store::at(&*root);
                        let requester = sources::requester(
                            source.as_str(),
                            "service:m4t2r1x0s9r8",
                            "Matrix source",
                        )
                        .unwrap();
                        let review = PermissionReview {
                            capability: "Write the exact report".into(),
                            targets: vec!["report".into()],
                            lifetime: "This write once".into(),
                            consequences: vec!["Changes the selected report".into()],
                            requester_reason: None,
                            exact_operation: "write(report,hello)".into(),
                        };
                        let operation = sources::operation(
                            source.clone(),
                            b"payload",
                            &review,
                            Scope::OperationOnce,
                            None,
                        )
                        .unwrap();
                        let effect = if domain.authorization {
                            Effect::Authorize(Box::new(operation.clone()))
                        } else {
                            Effect::ReplyOnly
                        };
                        let draft = QuestionDraft {
                            title: "Review report".into(),
                            questions: vec![QuestionItem {
                                id: ItemId::new("answer").unwrap(),
                                prompt: "Which report?".into(),
                                answer_shape: shape.clone(),
                                default_answer: None,
                            }],
                            caller_idempotency_key: "matrix".into(),
                            requested_timeout: Some(sources::timeout(
                                std::time::Duration::from_secs(30),
                            )),
                        };
                        let id = store.create_bound(
                            requester,
                            sources::owner(),
                            domain.id.clone(),
                            draft,
                            effect,
                            domain.authorization.then_some(review),
                            100,
                        );
                        if !accepts(source, &shape) {
                            assert!(id.is_err(), "unsupported source shape was silently reduced");
                            continue;
                        }
                        let id = id.unwrap();
                        let route = "owner:matrix";
                        let envelope = store
                            .plan_delivery(&id, route, "test", &declared.form(), None, 100)
                            .unwrap();
                        let text = match shape {
                            AnswerShape::Confirmation => "yes",
                            AnswerShape::Text { .. } => "Keep the detail precise",
                            AnswerShape::Choice {
                                max_selected: 2, ..
                            } => "1,2",
                            AnswerShape::Choice { .. } => "other A custom report",
                        };
                        let mut wrong = ingress(route);
                        wrong.actor = "user:o1t2h3e4r5u6".parse().unwrap();
                        assert!(
                            store
                                .submit(&wrong, candidate(&envelope, text, "wrong"), 101)
                                .is_err()
                        );
                        store
                            .submit(
                                &ingress(route),
                                candidate(&envelope, text, "owner-answer"),
                                101,
                            )
                            .unwrap();
                        let result = store.snapshot(&id, 101).unwrap();
                        assert_eq!(result.resolution.unwrap().trigger, Trigger::UserSubmission);
                        if domain.authorization {
                            let checks = SourceChecks {
                                operation,
                                authority_current: true,
                                generation_current: true,
                                control_fence_clear: true,
                            };
                            let claim = store.claim(&id, &checks, 101).unwrap();
                            assert!(store.claim(&id, &checks, 101).is_err());
                            store
                                .report(
                                    &id,
                                    source,
                                    &claim,
                                    EffectOutcome::Applied {
                                        detail: "matrix source confirmed its exact operation"
                                            .into(),
                                    },
                                    101,
                                )
                                .unwrap();
                        } else {
                            assert_eq!(result.effect, EffectState::None);
                        }
                        assert!(render::delivery(&envelope).contains(&envelope.response_code));
                        assert!(
                            !serde_json::to_string(&store.snapshot(&id, 101).unwrap())
                                .unwrap()
                                .contains(&envelope.response_code)
                        );
                    }
                }
            }
        }
    }
    assert_eq!(
        cells.len(),
        16,
        "a surface change needs an owning test chain or an explicit unsupported disposition"
    );
}

#[test]
fn unsupported_native_sets_and_inline_controls_require_a_standalone_fallback() {
    assert!(!surface("native", Host::Macos).unwrap().sets);
    assert!(surface("native", Host::Linux).unwrap().sets);
    for declaration in surfaces() {
        assert!(!declaration.inline);
    }
    assert!(surface("website", Host::Linux).is_none());
    assert!(surface("hosted", Host::Macos).is_none());
    assert_eq!(
        surface("native", Host::Linux)
            .unwrap()
            .form()
            .max_review_bytes,
        32 * 1024
    );
}
