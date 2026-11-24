//! Normal read receipts qualify only through an answered model request.

use crate::common;
use link_files::reading::ContentDigest;
use link_harness::agent::Agent;
use link_harness_wire::evidence::{
    Outcome, ReadAuthority, SkillAnchor, SkillRoute, SkillSource,
    index::{self, Located},
};
use serde_json::{Value, json};

mod fixture;
use fixture::{FACT, Mode};

#[tokio::test]
async fn normal_reads_require_trusted_complete_content_and_actual_model_inclusion() {
    for mode in [
        Mode::File,
        Mode::Memory,
        Mode::Repository,
        Mode::Partial,
        Mode::Clipped,
        Mode::Tampered,
        Mode::Unregistered,
    ] {
        let run = fixture::run(mode).await;
        assert_eq!(run.calls.len(), 1, "{mode:?}");
        assert_eq!(run.calls[0]["name"], "read");
        assert_eq!(run.calls[0]["arguments"]["path"], mode.path());
        assert_eq!(run.asked.len(), 2, "{mode:?}");
        assert!(
            !run.asked[0].to_string().contains(FACT),
            "skill body leaked into the initial prompt: {mode:?}"
        );
        let displayed = run.asked[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "tool")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        let trace = index::index_with_responses(
            run.state.events.iter().filter_map(|event| {
                event.evidence.as_ref().map(|evidence| Located {
                    seq: event.seq,
                    evidence,
                })
            }),
            run.state
                .events
                .iter()
                .filter_map(|event| event.request.map(|request| (event.seq, request))),
        )
        .unwrap();
        assert_eq!(trace.requests.len(), run.asked.len(), "{mode:?}");
        assert_eq!(trace.executions.len(), 1);
        let (call, execution) = trace.executions.iter().next().unwrap();
        assert_eq!(execution.proposed.call.arguments["path"], mode.path());
        let dispatch = execution.dispatch.unwrap();
        assert_eq!(
            dispatch.server,
            if mode == Mode::Memory {
                "memory"
            } else {
                "file"
            }
        );
        assert_eq!(dispatch.operation, run.calls[0]["name"].as_str().unwrap());
        assert_eq!(
            dispatch.arguments.retained(),
            Some(&run.calls[0]["arguments"])
        );
        let finished = execution.finished.as_ref().unwrap();
        let Outcome::Completed { reply } = finished.outcome else {
            panic!("read was not completed: {mode:?}");
        };
        assert!(!reply.is_error, "{mode:?}");
        let (producing, _) = trace.producing_request(*call).unwrap();
        if mode == Mode::Tampered {
            assert!(trace.skills.is_empty());
            assert!(!reply.model.omissions.is_empty());
            continue;
        }
        assert_eq!(trace.skills.len(), 1, "{mode:?}");
        let (loaded_at, load) = trace.skills[0];
        assert_eq!(load.anchor, SkillAnchor::Call { call: *call });
        assert_eq!(load.model_view, ContentDigest::of(displayed.as_bytes()));
        assert_eq!(
            load.read.source.content(),
            &ContentDigest::of(run.source.as_bytes())
        );
        assert_eq!(load.complete, mode.complete(), "{mode:?}: {load:?}");
        let supplied = trace.supplied_requests(loaded_at, load);
        if !mode.complete() {
            assert!(
                supplied.is_empty(),
                "an incomplete read received credit: {mode:?}"
            );
            assert!(!load.omissions.is_empty());
            if mode == Mode::Unregistered {
                assert!(matches!(
                    load.authority,
                    ReadAuthority::ServiceAsserted { .. }
                ));
                assert!(displayed.contains(FACT));
            }
            if mode == Mode::Clipped {
                assert!(reply.model.cut_text_bytes > 0);
                assert!(load.read.covers_source());
                assert!(load.supplied.is_none());
                assert!(!displayed.contains(FACT));
            }
            if mode == Mode::Partial {
                assert!(!load.read.covers_source());
                assert_eq!(reply.model.cut_text_bytes, 0);
            }
            continue;
        }
        assert!(matches!(load.authority, ReadAuthority::Managed { .. }));
        assert_eq!(supplied.len(), 1);
        assert!(producing < finished.seq && finished.seq < supplied[0]);
        assert!(trace.responses.contains_key(&supplied[0]));
        let projection = trace.projection(supplied[0]).unwrap();
        assert!(
            projection
                .messages
                .iter()
                .flat_map(|message| &message.answers)
                .any(|answer| answer.text.matches(displayed.as_bytes()))
        );
        assert!(displayed.contains(FACT));
        let expected = if mode == Mode::Memory {
            run.source.lines().collect::<Vec<_>>().join("\n")
        } else {
            run.source.clone()
        };
        assert_eq!(load.supplied, Some(ContentDigest::of(expected.as_bytes())));
        assert_eq!(
            load.route,
            if mode == Mode::Memory {
                SkillRoute::Memory
            } else {
                SkillRoute::File
            }
        );
        assert_eq!(
            load.source,
            if mode == Mode::Repository {
                SkillSource::Repository {
                    path: mode.path().into(),
                }
            } else {
                SkillSource::Owned {
                    path: mode.path().into(),
                }
            }
        );
        if mode == Mode::Memory {
            let document: Value = serde_json::from_str(displayed).unwrap();
            assert_eq!(document["text"], expected);
            assert!(document.get("read").is_none());
        } else {
            assert_eq!(displayed, expected);
        }
    }
}
