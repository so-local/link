use super::*;
use crate::*;

fn run(json: bool, f: impl FnOnce(&mut Out) -> Result<Done, Report>) -> (u8, Captured) {
    let (mut out, captured) = Out::capture(json, &["lnk-agent", "start", "--harness", "hermes"]);
    let result = f(&mut out);
    (out.finish(result), captured)
}

#[test]
fn an_activity_redraw_never_wraps_onto_another_row() {
    let line = "[23/40] Checking Billing export: under BigQuery export, turn on Standard usage cost to project example-development, dataset example_billing";
    for columns in [0, 1, 8, 20, 80, 160] {
        for clock in ["0:00", "10:00", "100:00"] {
            let shown = activity_frame(line, look::SPINNER[0], clock, columns);
            assert!(render::width(&shown) < columns.max(1), "{shown}");
            if columns >= 20 {
                assert!(shown.ends_with(clock), "{shown}");
            }
        }
    }
    assert_eq!(
        activity_frame(line, look::SPINNER[0], "0:00", 80),
        format!("{}...  {} 0:00", &line[..68], look::SPINNER[0])
    );
}

#[test]
fn activity_frames_keep_styles_when_the_description_fits() {
    let line = "Pulling \x1b[1mmy-model\x1b[0m";
    assert_eq!(
        activity_frame(line, look::SPINNER[0], "0:00", 80),
        format!("{line}  {} 0:00", look::SPINNER[0])
    );
    let shown = activity_frame(line, look::SPINNER[0], "0:00", 24);
    assert_eq!(shown, format!("Pulling my-m...  {} 0:00", look::SPINNER[0]));
    assert!(!shown.contains('\x1b'));
    let shown = activity_frame("Checking café data", look::SPINNER[0], "0:00", 25);
    assert_eq!(
        shown,
        format!("Checking café...  {} 0:00", look::SPINNER[0])
    );
}

fn start(out: &mut Out) -> Result<Done, Report> {
    let agent = Name::new("agent", "my-otter");
    let mut steps = Steps::new(2);
    out.step(
        &mut steps,
        phrase!("Pulling {}", Name::new("model", "qwen3:8b")),
    );
    out.step(&mut steps, phrase!("Starting {}", &agent));
    out.block(Fields::new().row("Name", &agent).key("Model", "qwen3:8b"));
    Ok(Done::success(sentence!("Started {}.", &agent))
        .next(cmd!("lnk agent show"), "see what it's doing"))
}

#[test]
fn a_pipe_gets_the_result_on_stdout_and_the_rest_on_stderr() {
    let (exit, c) = run(false, start);
    assert_eq!(exit, 0);
    assert_eq!(
        c.stdout(),
        "\u{2713} Started my-otter.\n\n  Name   my-otter\n  Model  qwen3:8b\n"
    );
    assert_eq!(
        c.stderr(),
        "[1/2] Pulling qwen3:8b...\n[2/2] Starting my-otter...\n\n  Next\n    lnk agent show   see what it's doing\n"
    );
}

#[test]
fn json_is_one_document_of_every_block() {
    let (exit, c) = run(true, start);
    assert_eq!(exit, 0);
    assert_eq!(c.stderr(), "");
    let doc: serde_json::Value = serde_json::from_str(&c.stdout()).unwrap();
    assert_eq!(doc["exit"], 0);
    let blocks = doc["blocks"].as_array().unwrap();
    assert_eq!(blocks[0]["progress"]["what"], "Pulling qwen3:8b");
    assert_eq!(blocks[2]["fields"][0]["value"]["agent"], "my-otter");
    assert_eq!(blocks[2]["fields"][1]["key"], true);
    assert_eq!(blocks[3]["success"]["done"], "Started my-otter.");
    assert_eq!(blocks[4]["next"][0]["command"][2], "show");
}

#[test]
fn a_report_ends_it_with_its_code() {
    let (exit, c) = run(false, |_| {
        Err(Report::refusal(
            Refuser::Sandbox,
            sentence!("The sandbox refused it."),
        ))
    });
    assert_eq!(exit, 3);
    assert_eq!(c.stdout(), "");
    assert!(c.stderr().starts_with("\u{2717} The sandbox refused it.\n"));
}

#[test]
fn only_links_own_fault_offers_lnk_issue() {
    fn adopted() -> Report {
        Report::from(anyhow::anyhow!("no such thing"))
    }
    let kinds: [(&str, Report, bool); 5] = [
        (
            "bug",
            Report::bug(sentence!("Something inside Link went wrong.")),
            true,
        ),
        ("unexpected", adopted(), true),
        (
            "person's",
            Report::error(sentence!("Couldn't start it.")),
            false,
        ),
        ("usage", adopted().exit(Exit::Usage), false),
        (
            "refusal",
            Report::refusal(Refuser::Sandbox, sentence!("The sandbox refused it.")),
            false,
        ),
    ];
    for (kind, report, offered) in kinds {
        let (exit, c) = run(false, |_| Err(report));
        assert!(exit > 0, "{kind}");
        assert_eq!(
            c.stderr().contains("lnk issue create"),
            offered,
            "{kind}: {}",
            c.stderr()
        );
        assert_eq!(
            c.stderr().contains(" \u{b7} lnk agent start"),
            offered,
            "{kind}"
        );
    }
}

#[test]
fn a_reply_escapes_what_could_hide_text() {
    let (_, c) = run(true, |out| {
        out.reply(&serde_json::json!({ "answer": "ok\u{202E}gpj\u{85}\u{200B}." }));
        Ok(Done::shown())
    });
    assert_eq!(c.stdout(), "{\"answer\":\"ok\\u202egpj\\u0085\\u200b.\"}\n");
    let back: serde_json::Value = serde_json::from_str(&c.stdout()).unwrap();
    assert_eq!(back["answer"], "ok\u{202E}gpj\u{85}\u{200B}.");
}

#[test]
fn a_piece_shows_at_once_with_its_newlines() {
    let (_, c) = run(false, |out| {
        out.live().piece("Hel");
        out.live().piece("lo\x1b[31m,\n\tthere\u{202E}");
        Ok(Done::shown())
    });
    assert_eq!(c.stdout(), "Hello[31m,\n\tthere");
}

#[test]
fn a_warning_shows_at_once_and_the_command_goes_on() {
    let (exit, c) = run(false, |out| {
        out.block(Report::warning(sentence!("It's small.")));
        Ok(Done::success(sentence!("Started it.")))
    });
    assert_eq!(exit, 0);
    assert_eq!(c.stderr(), "! It's small.\n");
}

#[test]
fn a_reply_is_what_json_prints() {
    let (exit, c) = run(true, |out| {
        out.reply(&serde_json::json!([{ "name": "qa-1" }]));
        out.block(Report::warning(sentence!("Couldn't ask one cloud.")));
        Ok(Done::shown())
    });
    assert_eq!(exit, 0);
    assert_eq!(c.stdout(), "[{\"name\":\"qa-1\"}]\n");
    assert_eq!(c.stderr(), "! Couldn't ask one cloud.\n");
    let (exit, c) = run(true, |out| {
        out.reply(&serde_json::json!([]));
        Err(Report::error(sentence!("Couldn't read your boxes.")))
    });
    assert_eq!(exit, 1);
    assert_eq!(c.stdout(), "");
    assert!(c.stderr().starts_with("\u{2717} Couldn't read your boxes."));
}

#[test]
fn the_command_typed_is_a_retry() {
    let (out, _) = Out::capture(
        false,
        &["/usr/bin/lnk-agent", "start", "--harness", "hermes"],
    );
    assert_eq!(out.typed().text(), "lnk agent start --harness hermes");
    assert_eq!(out.path, "lnk agent start");
    let (out, _) = Out::capture(false, &["lnk-box", "box", "create", "--cloud", "aws"]);
    assert_eq!(out.typed().text(), "lnk box create --cloud aws");
    assert_eq!(out.path, "lnk box create");
}

#[test]
fn a_private_reply_never_reaches_a_terminal() {
    let (mut out, c) = Out::capture(false, &["lnk-secret", "secret", "show"]);
    out.reply_private(&serde_json::json!({ "token": "t" }))
        .unwrap();
    assert_eq!(out.finish(Ok(Done::shown())), 0);
    assert_eq!(c.stdout(), "{\"token\":\"t\"}\n");
    let (mut out, _) = Out::capture(false, &["lnk-secret", "secret", "show"]);
    out.out_tty = true;
    let refused = out
        .reply_private(&serde_json::json!({ "token": "t" }))
        .unwrap_err();
    assert_eq!(refused.exit, Exit::Refused);
}

#[test]
fn an_inner_run_leaves_next_and_the_footer_to_the_outer() {
    let (mut out, c) = Out::capture(false, &["lnk-model", "model", "install"]);
    out.inner = true;
    let done = Done::success(sentence!("Installed it.")).next(cmd!("lnk model list"), "see it");
    assert_eq!(out.finish(Ok(done)), 0);
    assert_eq!(c.stderr(), "");
    let (mut out, c) = Out::capture(false, &["lnk-model", "model", "install"]);
    out.inner = true;
    assert_eq!(
        out.finish(Err(Report::error(sentence!("Couldn't install it.")))),
        1
    );
    assert_eq!(c.stderr(), "\u{2717} Couldn't install it.\n");
}

#[test]
fn a_failure_names_its_record() {
    let temp = link_test_kit::TempDir::new("out-records");
    let dir = temp.join("records");
    let (mut out, c) = Out::capture(false, &["lnk-agent", "start"]);
    out.records = Some(dir.clone());
    assert_eq!(out.finish(Err(Report::bug(sentence!("Couldn't.")))), 1);
    let said = c.stderr();
    let record = said.rsplit("everything is in ").next().unwrap().trim();
    let record = std::path::Path::new(record);
    let name = record.file_name().unwrap();
    assert!(dir.join(name).exists(), "{said}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_block_needed_now_shows_at_once() {
    let (mut out, c) = Out::capture(false, &["lnk-agent", "agent", "connect"]);
    out.now(Fields::new().key("Code", "ABCD-EFGH"));
    assert_eq!(c.stderr(), "\n  Code  ABCD-EFGH\n\n");
    assert_eq!(out.finish(Ok(Done::shown())), 0);
    assert_eq!(c.stdout(), "");
}

#[test]
fn a_reply_a_line_at_a_time_keeps_its_lines() {
    let (exit, c) = run(true, |out| {
        out.reply_line(&serde_json::json!({ "id": 1 }));
        out.block(Fields::new().row("Id", 1u64));
        out.reply_line(&serde_json::json!({ "id": 2 }));
        Ok(Done::shown())
    });
    assert_eq!(exit, 0);
    assert_eq!(c.stdout(), "{\"id\":1}\n{\"id\":2}\n");
    let (_, c) = run(false, |out| {
        out.reply_line(&serde_json::json!({ "id": 1 }));
        out.block(Fields::new().row("Id", 1u64));
        Ok(Done::shown())
    });
    assert_eq!(c.stdout(), "  Id  1\n");
}

#[test]
fn a_passed_code_is_the_exit_and_not_a_failure() {
    let (exit, c) = run(false, |_| Ok(Done::passed(42)));
    assert_eq!(
        (exit, c.stdout(), c.stderr()),
        (42, String::new(), String::new())
    );
    let (exit, c) = run(true, |_| Ok(Done::passed(3)));
    assert_eq!((exit, c.stdout()), (3, String::new()));
}

#[test]
fn a_warning_mid_run_shows_in_its_place() {
    let (exit, c) = run(false, |out| {
        out.block(Progress {
            step: None,
            of: None,
            what: phrase!("Starting it"),
        });
        out.block(Report::warning(sentence!("Its stop hook failed.")));
        out.block(Progress {
            step: None,
            of: None,
            what: phrase!("Waiting"),
        });
        Ok(Done::success(sentence!("Started it.")))
    });
    assert_eq!(exit, 0);
    assert_eq!(
        c.stderr(),
        "Starting it...\n! Its stop hook failed.\nWaiting...\n"
    );
}

#[test]
fn a_command_that_wrote_nothing_through_out_gets_nothing_added() {
    let (exit, c) = run(true, |_| Ok(Done::shown()));
    assert_eq!(exit, 0);
    assert_eq!(c.stdout(), "");
}

#[test]
fn a_block_needed_now_reaches_stderr_under_json_too() {
    let (mut out, c) = Out::capture(true, &["lnk-agent", "share", "--json"]);
    let in_ten = std::time::SystemTime::now() + std::time::Duration::from_secs(600);
    out.now(Pairing::new(
        sentence!("Mention the bot with this code."),
        "4FQ2",
        in_ten,
    ));
    assert!(c.stderr().contains("    4FQ2\n"), "{}", c.stderr());
    assert_eq!(out.finish(Ok(Done::success(sentence!("Shared it.")))), 0);
    let doc: serde_json::Value = serde_json::from_str(&c.stdout()).unwrap();
    assert_eq!(doc["blocks"][0]["pairing"]["code"], "4FQ2");
}

#[test]
fn notes_come_from_another_thread_in_their_place() {
    let (mut out, c) = Out::capture(false, &["lnk-agent", "use"]);
    out.block(Progress::new(phrase!("Starting it")));
    let notes = out.notes();
    std::thread::spawn(move || notes.note(sentence!("Brought in 3 conversations.")))
        .join()
        .unwrap();
    out.block(Progress::new(phrase!("Waiting")));
    assert_eq!(
        c.stderr(),
        "Starting it...\n  Brought in 3 conversations.\nWaiting...\n"
    );
}

#[test]
fn a_failure_takes_the_trace_its_inner_run_left() {
    let temp = link_test_kit::TempDir::new("carry");
    let records = temp.join("records");
    let _ = std::fs::remove_dir_all(&records);
    let dir = crate::inner::folder(&records);
    let inner = Trace {
        steps: vec![Step {
            what: "reading the box's address".into(),
            at: Some("modules/compute/box/src/list.rs:9".into()),
            program: None,
        }],
    };
    crate::inner::write(&dir, "lnk box list", &inner).unwrap();
    let (mut out, _) = Out::capture(false, &["lnk-agent", "start"]);
    out.records = Some(records.clone());
    let failed = out.carry(Err(Report::bug(sentence!("Couldn't list the boxes."))));
    let steps = failed.unwrap_err().trace.steps.clone();
    let _ = std::fs::remove_dir_all(&records);
    let last = steps.last().unwrap();
    assert_eq!(last.what, "reading the box's address");
    assert_eq!(last.program.as_deref(), Some("lnk box list"));
    assert!(steps[0].program.is_none(), "{steps:?}");
    assert!(!dir.exists());
}

#[test]
fn records_prepare_only_for_bugs_and_refused_preparation_writes_nothing() {
    fn prepare(dir: &std::path::Path) -> anyhow::Result<()> {
        link_files::create_private_dir(dir)?;
        Ok(())
    }
    fn refuse(_: &std::path::Path) -> anyhow::Result<()> {
        anyhow::bail!("the record destination is unavailable")
    }
    let temp = link_test_kit::TempDir::new("lazy-records");
    let dir = temp.join("logs");
    let (mut out, _) = Out::capture(false, &["lnk"]);
    out.records = Some(dir.clone());
    out.prepare_records = Some(prepare);
    assert!(!dir.exists());
    assert!(
        out.record(&Report::error(sentence!("A user error.")))
            .is_none()
    );
    assert!(!dir.exists());
    let bug = Report::bug(sentence!("An unexpected failure."));
    out.prepare_records = Some(refuse);
    assert!(out.record(&bug).is_none());
    assert!(!dir.exists());
    out.prepare_records = Some(prepare);
    assert!(out.record(&bug).unwrap().is_file());
}

#[test]
fn a_wait_with_no_terminal_says_it_is_still_going_until_it_ends() {
    let written: Arc<Mutex<Vec<u8>>> = Arc::default();
    let sink: Sink = written.clone();
    let text = || String::from_utf8_lossy(&written.lock().unwrap()).into_owned();
    let still = |text: &str| {
        text.lines()
            .filter(|l| l.starts_with("[2/3] Saving the disk, still going after "))
            .count()
    };
    let wait = Activity::still(
        "[2/3] Saving the disk".into(),
        sink,
        std::time::Duration::from_millis(10),
    );
    link_test_kit::wait::eventually(
        "two lines saying the wait is still going",
        std::time::Duration::from_secs(10),
        || still(&text()) >= 2,
    );
    drop(wait);
    // Dropping it joined its thread: nothing more is said after.
    let said = text();
    assert_eq!(said.lines().count(), still(&said), "{said}");
}
