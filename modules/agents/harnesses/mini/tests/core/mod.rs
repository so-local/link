use super::*;
use serde_json::json;
use std::future::ready;

fn call(id: &str) -> Call {
    Call {
        id: id.into(),
        name: "count".into(),
        arguments: json!({}),
    }
}
fn result(call: Call) -> Answer {
    Answer {
        call: call.id,
        text: "done".into(),
        failed: false,
        parts: Vec::new(),
    }
}

#[tokio::test]
async fn follows_more_than_eight_tool_rounds_without_adding_instructions() {
    let input = Message {
        role: "user".into(),
        text: "count".into(),
        ..Message::default()
    };
    let mut round = 0;
    let mut recorded = Vec::new();
    run_turn(
        vec![input.clone()],
        |messages| {
            assert_eq!(messages[0], input);
            assert!(messages.iter().all(|m| m.role != "system"));
            let n = round;
            round += 1;
            if n > 0 {
                assert_eq!(
                    messages.last().unwrap().answers[0].call,
                    format!("call-{}", n - 1)
                );
            }
            ready(Ok(Answered {
                text: if n == 10 {
                    "finished".into()
                } else {
                    String::new()
                },
                calls: if n == 10 {
                    Vec::new()
                } else {
                    vec![call(&format!("call-{n}"))]
                },
                ..Answered::default()
            }))
        },
        |call| ready(result(call)),
        |message| {
            recorded.push(message.clone());
            Ok(())
        },
    )
    .await
    .unwrap();
    assert_eq!(round, 11);
    assert_eq!(recorded.len(), 21);
    assert_eq!(recorded.last().unwrap().text, "finished");
}

#[tokio::test]
async fn portable_tool_turn_preserves_every_message_and_thinking_block() {
    #[derive(serde::Deserialize)]
    struct Example {
        messages: Vec<Message>,
        replies: Vec<Message>,
        results: Vec<Answer>,
        expected: Vec<Message>,
    }
    let example: Example =
        serde_json::from_str(include_str!("../fixtures/tool-turn.json")).unwrap();
    let mut replies = example.replies.into_iter();
    let mut results = example.results.into_iter();
    let mut saved = example.messages.clone();
    run_turn(
        example.messages,
        |_| {
            let next = replies.next().unwrap();
            ready(Ok(Answered {
                text: next.text,
                calls: next.calls,
                thinking: next.thinking,
                ..Answered::default()
            }))
        },
        |call| {
            let answer = results.next().unwrap();
            assert_eq!(answer.call, call.id);
            ready(answer)
        },
        |message| {
            saved.push(message.clone());
            Ok(())
        },
    )
    .await
    .unwrap();
    assert_eq!(saved, example.expected);
}

#[tokio::test]
async fn returns_model_errors_without_retrying_or_running_tools() {
    let mut asks = 0;
    let error = run_turn(
        Vec::new(),
        |_| {
            asks += 1;
            ready(Err(anyhow::anyhow!("model unavailable")))
        },
        |_| async { panic!("no tool was requested") },
        |_| panic!("no answer was received"),
    )
    .await
    .unwrap_err();
    assert_eq!(asks, 1);
    assert_eq!(error.to_string(), "model unavailable");
}

#[tokio::test]
async fn a_recording_failure_stops_before_a_tools_side_effect() {
    let error = run_turn(
        Vec::new(),
        |_| {
            ready(Ok(Answered {
                calls: vec![call("one")],
                ..Answered::default()
            }))
        },
        |_| async { panic!("the call was not recorded") },
        |_| anyhow::bail!("storage refused"),
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "storage refused");
}

#[tokio::test]
async fn duplicate_call_ids_are_refused_before_execution() {
    let error = run_turn(
        Vec::new(),
        |_| {
            ready(Ok(Answered {
                calls: vec![call("one"), call("one")],
                ..Answered::default()
            }))
        },
        |_| async { panic!("duplicate call") },
        |_| panic!("invalid reply should not be recorded"),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("unique"));
}

#[tokio::test]
async fn tool_failures_are_recorded_and_given_back_to_the_model() {
    let mut asks = 0;
    let mut saved = Vec::new();
    run_turn(
        Vec::new(),
        |messages| {
            asks += 1;
            ready(if asks == 1 {
                Ok(Answered {
                    calls: vec![call("one")],
                    ..Answered::default()
                })
            } else {
                assert!(messages.last().unwrap().answers[0].failed);
                Ok(Answered {
                    text: "The file is unavailable.".into(),
                    ..Answered::default()
                })
            })
        },
        |call| {
            ready(Answer {
                call: call.id,
                text: "not found".into(),
                failed: true,
                parts: Vec::new(),
            })
        },
        |message| {
            saved.push(message.clone());
            Ok(())
        },
    )
    .await
    .unwrap();
    assert_eq!(asks, 2);
    assert_eq!(saved[1].answers[0].text, "not found");
}
