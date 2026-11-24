use serde_json::json;

use super::*;

fn paths(parts: &[Part]) -> Vec<&str> {
    parts.iter().map(|p| p.path.as_str()).collect()
}

fn part<'a>(parts: &'a [Part], path: &str) -> &'a Part {
    parts.iter().find(|p| p.path == path).expect(path)
}

#[test]
fn anthropic_names_tools_calls_and_results() {
    let body = json!({
        "model": "claude-sonnet-5-5",
        "system": [{"type": "text", "text": "Be brief."}],
        "tools": [{"name": "read", "input_schema": {}}, {"name": "run", "input_schema": {}}],
        "messages": [
            {"role": "user", "content": "Read a"},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "I should read it", "signature": "s"},
                {"type": "tool_use", "id": "t1", "name": "read", "input": {"path": "a"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": [
                    {"type": "text", "text": "hello"},
                    {"type": "image", "source": {"type": "base64", "data": "AAAA"}}
                ]},
                {"type": "image", "source": {"type": "base64", "data": "BBBBBB"}}
            ]}
        ]
    });
    let parts = census(Api::Anthropic, &body);
    assert_eq!(
        paths(&parts),
        [
            "tools/read",
            "tools/run",
            "instructions",
            "conversation/user",
            "conversation/thinking",
            "conversation/calls/read",
            "conversation/results/read",
            "conversation/media/image",
        ]
    );
    assert_eq!(part(&parts, "instructions").bytes, 9);
    // The result's image is the tool's, the user's own is media.
    let result = part(&parts, "conversation/results/read");
    assert_eq!(result.bytes, 5 + 4);
    assert_eq!(result.weight, 1 + IMAGE_TOKENS as u64);
    assert_eq!(part(&parts, "conversation/media/image").bytes, 6);
}

#[test]
fn chat_completions_names_results_by_their_call() {
    let body = json!({
        "messages": [
            {"role": "system", "content": "Be brief."},
            {"role": "user", "content": [{"type": "text", "text": "Run ls"}]},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "run", "arguments": "{\"cmd\":\"ls\"}"}}
            ]},
            {"role": "tool", "tool_call_id": "c1", "content": "a b"},
            {"role": "tool", "tool_call_id": "unknown", "content": "x"}
        ],
        "tools": [{"type": "function", "function": {"name": "run", "parameters": {}}}]
    });
    let parts = census(Api::Chat, &body);
    assert_eq!(
        paths(&parts),
        [
            "tools/run",
            "instructions",
            "conversation/user",
            "conversation/calls/run",
            "conversation/results/run",
            "conversation/results/other",
        ]
    );
    assert_eq!(part(&parts, "conversation/results/run").bytes, 3);
}

#[test]
fn responses_reads_instructions_items_and_reasoning() {
    let body = json!({
        "instructions": "Be brief.",
        "tools": [{"type": "function", "name": "read"}, {"type": "web_search"}],
        "input": [
            {"role": "user", "content": [{"type": "input_text", "text": "Read a"}]},
            {"type": "reasoning", "summary": [{"type": "summary_text", "text": "read it"}]},
            {"type": "function_call", "call_id": "f1", "name": "read", "arguments": "{}"},
            {"type": "function_call_output", "call_id": "f1", "output": "hello"}
        ]
    });
    let parts = census(Api::Responses, &body);
    assert_eq!(
        paths(&parts),
        [
            "instructions",
            "tools/read",
            "tools/web_search",
            "conversation/user",
            "conversation/thinking",
            "conversation/calls/read",
            "conversation/results/read",
        ]
    );
}

#[test]
fn ollama_and_completions() {
    let chat = json!({"messages": [
        {"role": "user", "content": "hi", "images": ["AAAA"]},
        {"role": "assistant", "content": "", "tool_calls": [{"function": {"name": "run", "arguments": {}}}]},
        {"role": "tool", "tool_name": "run", "content": "ok"}
    ]});
    assert_eq!(
        paths(&census(Api::OllamaChat, &chat)),
        [
            "conversation/user",
            "conversation/media/image",
            "conversation/calls/run",
            "conversation/results/run",
        ]
    );
    let generate = json!({"system": "Be brief.", "prompt": "hi"});
    assert_eq!(
        paths(&census(Api::OllamaGenerate, &generate)),
        ["instructions", "conversation/user"]
    );
    assert_eq!(
        paths(&census(Api::Completions, &json!({"prompt": "hi"}))),
        ["conversation/user"]
    );
}

#[test]
fn a_name_that_isnt_plain_is_other_and_parts_are_bounded() {
    let mut tools: Vec<_> = (0..PARTS_MAX + 10)
        .map(|i| json!({"name": format!("tool{i}")}))
        .collect();
    tools.push(json!({"name": "has space"}));
    let parts = census(Api::Anthropic, &json!({"tools": tools}));
    assert!(parts.len() <= PARTS_MAX);
    let other = part(&parts, "tools/other");
    assert!(other.weight > 0);
    let total: u64 = parts.iter().map(|p| p.bytes).sum();
    let sent: usize = tools.iter().map(|t| t.to_string().len()).sum();
    assert_eq!(total, sent as u64, "every tool counted once");
}

#[test]
fn opening_is_the_first_persons_message() {
    let one = json!({"messages": [{"role": "system", "content": "x"}, {"role": "user", "content": "hi"}]});
    let later = json!({"messages": [
        {"role": "user", "content": "hi"},
        {"role": "assistant", "content": "hello"},
        {"role": "user", "content": "more"}
    ]});
    let other = json!({"messages": [{"role": "user", "content": "bye"}]});
    let of = |body| opening(Api::Chat, body);
    assert_eq!(of(&one), of(&later));
    assert_ne!(of(&one), of(&other));
    assert_eq!(of(&json!({})), None);
}

fn call(input: Option<u64>, weights: &[(&str, u64)]) -> Call {
    Call {
        seq: 1,
        tokens: input.map(|input| crate::chat::Tokens {
            input,
            output: 1,
            cached: 0,
        }),
        parts: weights
            .iter()
            .map(|&(path, weight)| Part {
                path: path.into(),
                bytes: weight * 4,
                weight,
            })
            .collect(),
        ..Call::default()
    }
}

#[test]
fn measured_input_is_split_by_weight_exactly() {
    let shares = call(Some(100), &[("a", 1), ("b", 1), ("c", 1)]).shares();
    let tokens: Vec<u64> = shares.iter().map(|s| s.tokens).collect();
    assert_eq!(tokens, [34, 33, 33]);
    assert!(shares.iter().all(|s| s.how == How::Apportioned));
    let shares = call(Some(1000), &[("a", 3), ("b", 1)]).shares();
    assert_eq!(shares[0].tokens, 750);
    assert_eq!(shares[1].tokens, 250);
}

#[test]
fn without_usage_the_weights_stand() {
    let shares = call(None, &[("a", 7), ("b", 2)]).shares();
    assert_eq!(shares[0].tokens, 7);
    assert!(shares.iter().all(|s| s.how == How::Estimated));
}

#[test]
fn a_report_sums_each_part_over_the_chat() {
    let first = call(Some(10), &[("instructions", 1), ("conversation/user", 1)]);
    let mut second = call(None, &[("instructions", 5), ("tools/run", 5)]);
    second.seq = 2;
    let report = report("main", &[first, second]).expect("calls");
    assert_eq!(report.calls, 2);
    assert_eq!(report.latest.seq, 2);
    let instructions = &report.over[0];
    assert_eq!(
        (instructions.path.as_str(), instructions.tokens),
        ("instructions", 10)
    );
    assert_eq!(instructions.how, How::Estimated, "one call wasn't measured");
    assert_eq!(report.over.len(), 3);
    assert_eq!(super::report("main", &[]), None);
}

#[test]
fn a_composed_system_prompt_is_split_by_its_spans() {
    let system = "Base words. # Skills\n- tides: Reads the tides.\nYou are Ada.";
    let at = |text: &str| system.find(text).unwrap() as u64;
    let span = |name: &str, text: &str| Span {
        name: name.into(),
        start: at(text),
        end: at(text) + text.len() as u64,
    };
    let spans = [
        span("base", "Base words."),
        span("skills", "# Skills\n"),
        span("skills/tides", "- tides: Reads the tides.\n"),
        span("persona", "You are Ada."),
    ];
    let parts = split(system, &spans).unwrap();
    assert_eq!(
        paths(&parts),
        [
            "instructions/base",
            "instructions",
            "instructions/skills",
            "instructions/skills/tides",
            "instructions/persona",
        ]
    );
    let bytes: u64 = parts.iter().map(|p| p.bytes).sum();
    assert_eq!(bytes, system.len() as u64, "every byte once");
    // Out of order, overlapping, past the end, or not plain: none.
    for bad in [
        vec![spans[1].clone(), spans[0].clone()],
        vec![span("a", "Base words. #"), span("b", "# Skills")],
        vec![Span {
            name: "a".into(),
            start: 0,
            end: 999,
        }],
        vec![Span {
            name: "../a".into(),
            start: 0,
            end: 1,
        }],
    ] {
        assert_eq!(split(system, &bad), None, "{bad:?}");
    }
    // Inside a character: none.
    assert_eq!(
        split(
            "é",
            &[Span {
                name: "a".into(),
                start: 0,
                end: 1
            }]
        ),
        None
    );
}

#[test]
fn an_agents_records_are_forgotten_with_it() {
    let dir = link_test_kit::TempDir::new("context-forget");
    for agent in ["work", "main"] {
        std::fs::create_dir_all(super::dir(&dir, agent).unwrap()).unwrap();
    }
    forget(&dir, "work").unwrap();
    assert!(!super::dir(&dir, "work").unwrap().exists());
    assert!(super::dir(&dir, "main").unwrap().exists(), "another's stay");
    forget(&dir, "work").unwrap();
    assert!(forget(&dir, "../main").is_err());
}

#[test]
fn each_part_is_priced_as_its_share_of_the_prompt() {
    let mut priced = call(Some(1000), &[("tools/run", 1), ("conversation/user", 3)]);
    priced.tokens.as_mut().unwrap().cached = 400;
    priced.tokens.as_mut().unwrap().output = 100;
    priced.price = Some(crate::Price {
        input: 3.0,
        output: 15.0,
        cached: Some(0.3),
    });
    let shares = priced.shares();
    let prompt = (600.0 * 3.0 + 400.0 * 0.3) / 1e6;
    assert!((shares[0].cost.unwrap() - prompt / 4.0).abs() < 1e-12);
    assert!((shares[1].cost.unwrap() - prompt * 3.0 / 4.0).abs() < 1e-12);
    assert!((priced.snapshot().answer_cost.unwrap() - 100.0 * 15.0 / 1e6).abs() < 1e-12);
    // Over the chat, unknown once a call's price is.
    let unpriced = call(Some(10), &[("tools/run", 1)]);
    let over = report("main", &[priced.clone(), unpriced]).unwrap().over;
    assert_eq!(over[0].cost, None);
}

/// A call past its model's price jump is priced at the jump's price,
/// its prompt and its answer; one at the jump, at the first.
#[test]
fn a_call_past_the_jump_is_priced_higher() {
    let jumps = |input| {
        let mut call = call(Some(input), &[("conversation/user", 1)]);
        call.tokens.as_mut().unwrap().output = 100;
        call.price = Some(crate::Price {
            input: 0.1,
            output: 0.5,
            cached: None,
        });
        call.above = vec![crate::Jump {
            tokens: 1000,
            price: crate::Price {
                input: 0.5,
                output: 2.5,
                cached: None,
            },
        }];
        let prompt = call.shares()[0].cost.unwrap();
        (prompt, call.snapshot().answer_cost.unwrap())
    };
    let (prompt, answer) = jumps(1000);
    assert!((prompt - 1000.0 * 0.1 / 1e6).abs() < 1e-12);
    assert!((answer - 100.0 * 0.5 / 1e6).abs() < 1e-12);
    let (prompt, answer) = jumps(1001);
    assert!((prompt - 1001.0 * 0.5 / 1e6).abs() < 1e-12);
    assert!((answer - 100.0 * 2.5 / 1e6).abs() < 1e-12);
}

#[test]
fn a_chat_counts_the_work_it_asked_for() {
    let mut parent = report("main", &[call(Some(10), &[("instructions", 1)])]).unwrap();
    let child = report(
        "task-abc",
        &[call(Some(4), &[("instructions", 1), ("tools/run", 1)])],
    )
    .unwrap();
    parent.adopt(&adopted_under("main", "task-abc"), &child);
    let compaction = report("main/compaction", &[call(Some(6), &[("instructions", 1)])]).unwrap();
    parent.adopt(&adopted_under("main", "main/compaction"), &compaction);
    let over: Vec<(&str, u64)> = parent
        .over
        .iter()
        .map(|s| (s.path.as_str(), s.tokens))
        .collect();
    assert_eq!(
        over,
        [
            ("instructions", 10),
            ("subagents/task-abc/instructions", 2),
            ("subagents/task-abc/tools/run", 2),
            ("compaction/instructions", 6),
        ]
    );
    assert_eq!(adopted_under("main", "main/"), "subagents/main/");
}
