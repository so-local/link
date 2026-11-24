//! Every block's example from the approved look, held in its three
//! renderings: a terminal's, a pipe's and `--json`'s.

link_test_kit::program!(unit);

use std::time::{Duration, SystemTime};

use link_format::look::{Mark, Settings, Symbols};
use link_format::render::{self, Ctx, Footer};
use link_format::*;

fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000)
}

fn plain() -> Ctx {
    Ctx {
        home: Some("/home/me".into()),
        now: now(),
        ..Ctx::plain()
    }
}

fn terminal() -> Ctx {
    Ctx {
        home: Some("/home/me".into()),
        now: now(),
        ..Ctx::terminal(&Settings::default(), 80)
    }
}

fn bold(text: &str) -> String {
    format!("\x1b[1m{text}\x1b[0m")
}

fn tone(code: u8, text: &str) -> String {
    format!("\x1b[{code}m{text}\x1b[0m")
}

/// The block in each rendering: a pipe's, a terminal's and its JSON.
fn shown(b: impl Into<Block>) -> (String, String, serde_json::Value) {
    let b = b.into();
    (
        render::block(&b, &plain()),
        render::block(&b, &terminal()),
        serde_json::to_value(&b).unwrap(),
    )
}

states! {
    enum AgentState {
        Running("running") => Good,
        Stopped("stopped") => Quiet,
        Failed("failed") => Bad,
    }
}

#[test]
fn success_leads_with_its_mark() {
    let agent = Name::new("agent", "main");
    let (pipe, term, json) = shown(
        Success::new(sentence!("Started {}.", &agent))
            .changed("settings", Path::new("/home/me/Link/Safe/settings.toml")),
    );
    assert_eq!(
        pipe,
        "\u{2713} Started main.\n  settings: ~/Link/Safe/settings.toml\n"
    );
    assert_eq!(
        term,
        "\u{2705} Started main.\n  settings: ~/Link/Safe/settings.toml\n"
    );
    assert_eq!(json["success"]["done"], "Started main.");
    assert_eq!(
        json["success"]["changed"][0]["kept"]["path"],
        "/home/me/Link/Safe/settings.toml"
    );
}

#[test]
fn waiting_leads_with_its_own_mark_never_successs_or_a_failures() {
    let mut waiting = Success::new(sentence!("Made 3; the rest waits on you."));
    waiting.waiting = true;
    let (pipe, term, json) = shown(waiting);
    assert_eq!(pipe, "\u{2026} Made 3; the rest waits on you.\n");
    assert!(term.starts_with("\u{231b} "), "{term:?}");
    assert_eq!(json["success"]["waiting"], true);
    let (_, _, json) = shown(Success::new(sentence!("Made 3.")));
    assert!(json["success"].get("waiting").is_none(), "absent when done");
}

#[test]
fn fields_line_up_and_bold_only_the_key() {
    let (pipe, term, json) = shown(
        Fields::new()
            .row("Name", Name::new("agent", "main"))
            .key("Model", "qwen3:8b on Ollama")
            .row("Harness", "hermes"),
    );
    assert_eq!(
        pipe,
        "  Name     main\n  Model    qwen3:8b on Ollama\n  Harness  hermes\n"
    );
    assert_eq!(
        term,
        format!(
            "  Name     main\n  Model    {}\n  Harness  hermes\n",
            bold("qwen3:8b on Ollama")
        )
    );
    assert_eq!(
        json,
        serde_json::json!({ "fields": [
            { "label": "Name", "value": { "agent": "main" } },
            { "label": "Model", "value": "qwen3:8b on Ollama", "key": true },
            { "label": "Harness", "value": "hermes" },
        ]})
    );
}

fn agents() -> Table {
    let mut t = Table::new(["Name", "State", "Size"], sentence!("No agents yet.")).right(2);
    t.row::<Value>([
        Name::new("agent", "main").into(),
        AgentState::Running.into(),
        Size(4_000_000_000).into(),
    ]);
    t.row::<Value>([
        Name::new("agent", "work").into(),
        AgentState::Failed.into(),
        Size(512_000_000).into(),
    ]);
    t
}

#[test]
fn a_table_colors_only_its_states() {
    let (pipe, term, json) = shown(agents());
    assert_eq!(
        pipe,
        "  NAME  STATE      SIZE\n  main  running    4 GB\n  work  failed   512 MB\n"
    );
    assert_eq!(
        term,
        format!(
            "  NAME  STATE      SIZE\n  main  {}    4 GB\n  work  {}   512 MB\n",
            tone(32, "running"),
            tone(31, "failed")
        )
    );
    assert_eq!(json["table"]["columns"][2]["align"], "right");
    assert_eq!(
        json["table"]["rows"][1][1],
        serde_json::json!({ "state": "failed", "tone": "bad" })
    );
    assert_eq!(
        json["table"]["rows"][1][2],
        serde_json::json!({ "bytes": 512_000_000 })
    );
}

#[test]
fn an_empty_table_says_what_is_missing() {
    let t = Table::new(["Name"], sentence!("No agents yet."));
    let (pipe, term, json) = shown(t);
    assert_eq!(pipe, "  No agents yet.\n");
    assert_eq!(term, pipe);
    assert_eq!(json["table"]["empty"], "No agents yet.");
}

#[test]
fn a_table_wider_than_its_terminal_keeps_every_value_as_fields() {
    let mut t = Table::new(["Name", "About"], sentence!("No tools."));
    t.row([
        "web",
        "Reads pages on the open web, and follows the links it finds there",
    ]);
    let ctx = Ctx {
        width: 60,
        ..terminal()
    };
    let shown = render::block(&t.clone().into(), &ctx);
    assert_eq!(
        shown,
        "  Name   web\n  About  Reads pages on the open web, and follows the links it finds there\n"
    );
    assert!(render::block(&t.into(), &plain()).ends_with("the links it finds there\n"));
}

#[test]
fn tables_use_the_full_terminal_width_and_never_hide_long_steps() {
    let step = "BigQuery billing export for the development project";
    let why = "BigQuery error: Access Denied: billing dataset requires permission";
    let mut table = Table::new(
        ["Part", "Kind", "Step", "State", "Why"],
        sentence!("No steps."),
    );
    table.row(["project", "configuration", step, "unread", why]);
    let wide = Ctx::terminal(&Settings::default(), 180);
    let shown = render::block(&table.clone().into(), &wide);
    assert!(shown.contains("STEP"), "{shown}");
    assert!(shown.contains(step) && shown.contains(why), "{shown}");
    assert!(shown.lines().all(|line| line.chars().count() <= 180));
    let narrow = Ctx::terminal(&Settings::default(), 80);
    let shown = render::block(&table.into(), &narrow);
    assert!(shown.contains("Step "), "{shown}");
    assert!(shown.contains(step) && shown.contains(why), "{shown}");
}

#[test]
fn a_tree_draws_its_branches() {
    let tree = Tree::new(sentence!("Empty.")).root(
        Node::new(["photos/"])
            .child(Node::new(["2026/"]).child(Node::new(["a.jpg"])))
            .child(Node::new(["b.jpg"])),
    );
    let (pipe, term, json) = shown(tree);
    assert_eq!(
        pipe,
        "  photos/\n  \u{251c}\u{2500}\u{2500} 2026/\n  \u{2502}   \u{2514}\u{2500}\u{2500} a.jpg\n  \u{2514}\u{2500}\u{2500} b.jpg\n"
    );
    assert_eq!(term, pipe);
    assert_eq!(json["tree"]["roots"][0]["children"][1]["cells"][0], "b.jpg");
}

#[test]
fn text_wraps_on_a_terminal_and_never_in_a_pipe() {
    let words = "Each agent runs in its own sandbox, which lets it read and write only \
                 its own folder and reach only the hosts its spec allows.";
    let (pipe, term, json) = shown(Prose::from(words));
    assert_eq!(pipe, format!("  {words}\n"));
    assert!(term.lines().all(|l| l.chars().count() <= 80));
    assert!(term.lines().skip(1).all(|l| l.starts_with("  ")));
    assert_eq!(json["text"], words);
}

#[test]
fn a_section_bolds_its_heading_over_its_blocks() {
    let s = Section::new("Tools").with(Fields::new().row("web", "allowed"));
    let (pipe, term, json) = shown(s);
    assert_eq!(pipe, "  Tools\n    web  allowed\n");
    assert_eq!(term, format!("  {}\n    web  allowed\n", bold("Tools")));
    assert_eq!(json["section"]["heading"], "Tools");
}

#[test]
fn a_tail_indents_another_programs_lines() {
    let t = Tail::last("first\n\nlast line\n", 5).log("/home/me/.lnk/logs/main.log");
    let (pipe, term, json) = shown(t);
    assert_eq!(
        pipe,
        "    first\n    last line\n  Its log: ~/.lnk/logs/main.log\n"
    );
    assert_eq!(term, pipe);
    assert_eq!(json["tail"]["log"], "/home/me/.lnk/logs/main.log");
}

fn next() -> Next {
    let mut n = Next::default();
    n.push(
        cmd!("lnk agent connect telegram"),
        "text it from your phone",
    );
    n.push(cmd!("lnk agent show"), "see what it's doing");
    n
}

#[test]
fn next_bolds_its_commands_and_lines_up_their_whys() {
    let (pipe, term, json) = shown(next());
    assert_eq!(
        pipe,
        "  Next\n    lnk agent connect telegram   text it from your phone\n    \
         lnk agent show               see what it's doing\n"
    );
    assert_eq!(
        term,
        format!(
            "  {}\n    {}   text it from your phone\n    {}               see what it's doing\n",
            bold("Next"),
            bold("lnk agent connect telegram"),
            bold("lnk agent show")
        )
    );
    assert_eq!(json["next"][1]["command"][2], "show");
    assert_eq!(json["next"][1]["why"], "see what it's doing");
}

#[test]
fn a_report_has_its_why_tail_next_and_context() {
    let mut r = Report::bug(sentence!("Couldn't start {}.", Name::new("agent", "main")))
        .why(prose!("Ollama didn't answer at {}.", Address::local(11434)))
        .tail(Tail::last("Error: could not connect to ollama", 3))
        .next(cmd!("lnk model show"), "see whether Ollama is up");
    r.trace.steps[0].at = Some("agent/src/launching.rs:88".into());
    let footer = Footer {
        version: "lnk 0.9.2".into(),
        platform: "linux x86_64".into(),
        command: "lnk agent start".into(),
        record: Some("/home/me/Link/Logs/r.txt".into()),
    };
    let pipe = render::report(&r, Some(&footer), &plain());
    assert_eq!(
        pipe,
        "\u{2717} Couldn't start main.\n  Ollama didn't answer at localhost:11434.\n\n    \
         Error: could not connect to ollama\n\n  Next\n    lnk model show   see whether \
         Ollama is up\n\n  lnk 0.9.2 \u{b7} linux x86_64 \u{b7} lnk agent start \u{b7} \
         agent/src/launching.rs:88\n  Report it: lnk issue create \u{b7} everything is in \
         ~/Link/Logs/r.txt\n"
    );
    let term = render::report(&r, Some(&footer), &terminal());
    assert!(term.starts_with(&format!("\u{274c} {}\n", bold("Couldn't start main."))));
    let json = serde_json::to_value(Block::from(r)).unwrap();
    assert_eq!(json["report"]["kind"], "error");
    assert_eq!(json["report"]["exit"], 1);
    assert_eq!(
        json["report"]["trace"][0]["at"],
        "agent/src/launching.rs:88"
    );
}

#[test]
fn each_report_kind_has_its_mark_in_each_symbol_setting() {
    let kinds = [
        (
            Report::error(sentence!("Couldn't.")),
            ["\u{274c}", "\u{2717}", "error:"],
        ),
        (
            Report::warning(sentence!("It's small.")),
            ["\u{26a0}\u{fe0f} ", "!", "warning:"],
        ),
        (
            Report::refusal(Refuser::Sandbox, sentence!("The sandbox refused it.")),
            ["\u{26d4}", "\u{2717}", "refused:"],
        ),
    ];
    for (r, marks) in kinds {
        for (symbols, mark) in [Symbols::Emoji, Symbols::Plain, Symbols::None]
            .iter()
            .zip(marks)
        {
            let ctx = Ctx {
                symbols: *symbols,
                ..plain()
            };
            let shown = render::report(&r, None, &ctx);
            assert!(shown.starts_with(&format!("{mark} ")), "{shown:?}");
        }
    }
}

#[test]
fn progress_counts_its_steps() {
    let p = Progress::new(phrase!("Pulling {}", Name::new("model", "qwen3:8b"))).of(2, 3);
    let (pipe, term, json) = shown(p);
    assert_eq!(pipe, "[2/3] Pulling qwen3:8b...\n");
    assert_eq!(term, pipe);
    assert_eq!(
        json,
        serde_json::json!({ "progress": { "step": 2, "of": 3, "what": "Pulling qwen3:8b" }})
    );
}

#[test]
fn a_note_is_a_plain_line() {
    let (pipe, term, json) = shown(Note(sentence!("Tagged your box as yours in its cloud.")));
    assert_eq!(pipe, "  Tagged your box as yours in its cloud.\n");
    assert_eq!(term, pipe);
    assert_eq!(json["note"], "Tagged your box as yours in its cloud.");
}

#[test]
fn verbatim_lines_and_a_pairing_code() {
    let (pipe, term, json) = shown(
        Verbatim::new()
            .line("\"git\",")
            .commented("\"python3\",", "an interpreter"),
    );
    assert_eq!(pipe, "  \"git\",\n  \"python3\",  # an interpreter\n");
    assert_eq!(term, pipe);
    assert_eq!(json["verbatim"]["lines"][1]["comment"], "an interpreter");
    let (pipe, _, json) = shown(Verbatim::new().lead("Add them").line("\"git\","));
    assert_eq!(pipe, "  Add them:\n\n  \"git\",\n");
    assert_eq!(json["verbatim"]["lead"], "Add them");
    let p = Pairing::new(
        sentence!("In the chat, mention the bot with this code."),
        "4FQ2",
        now() + Duration::from_secs(600),
    );
    let (pipe, term, json) = shown(p);
    assert_eq!(
        pipe,
        "  In the chat, mention the bot with this code.\n\n    4FQ2\n\n  It expires in 10 min.\n"
    );
    // Plain, so it copies as it is.
    assert!(term.contains("    4FQ2\n") && !term.contains(&bold("4FQ2")));
    assert_eq!(json["pairing"]["code"], "4FQ2");
}

#[test]
fn marks_as_values_in_each_symbol_setting() {
    for (symbols, shown) in [
        (Symbols::Emoji, "\u{2713}"),
        (Symbols::Plain, "\u{2713}"),
        (Symbols::None, "ok"),
    ] {
        let ctx = Ctx { symbols, ..plain() };
        assert_eq!(render::value(&Mark::Success.into(), &ctx), shown);
    }
}

#[test]
fn never_emphasis_keeps_color_and_never_color_keeps_bold() {
    let words = |pairs: &'static [(&'static str, &'static str)]| {
        Settings::from(move |n| {
            pairs
                .iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| v.to_string())
        })
    };
    let no_bold = Ctx::terminal(&words(&[("LNK_OUTPUT_EMPHASIS", "never")]), 80);
    let no_color = Ctx::terminal(&words(&[("LNK_OUTPUT_COLOR", "never")]), 80);
    let b: Block = next().into();
    assert!(!render::block(&b, &no_bold).contains("\x1b[1m"));
    assert!(render::block(&b, &no_color).contains("\x1b[1m"));
    let t: Block = agents().into();
    assert!(render::block(&t, &no_bold).contains("\x1b[32m"));
    assert!(!render::block(&t, &no_color).contains("\x1b[32m"));
}
