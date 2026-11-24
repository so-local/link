use super::*;
use serde_json::json;

fn case_record(seconds: Option<f64>, skipped: bool) -> Program {
    Program {
        package: Some("example".into()),
        group: "example".into(),
        target: "case <one>".into(),
        executable: None,
        outcome: Some(if skipped { "skip" } else { "pass" }.into()),
        seconds,
        passed: u64::from(!skipped),
        failed: 0,
        ignored: u64::from(skipped),
        filtered: 0,
        failures: Vec::new(),
    }
}

#[test]
fn case_adapters_keep_known_counts_when_execution_time_is_unknown() {
    let report = measured(
        MeasurementKind::Cases,
        success(),
        vec![case_record(Some(2.0), false), case_record(None, true)],
        vec![],
    );
    assert_eq!(report.groups[0].passed, 1);
    assert_eq!(report.groups[0].skipped, 1);
    assert_eq!(report.groups[0].incomplete, 0);
    assert_eq!(report.groups[0].unmeasured, 1);
    assert!(!report.complete);
    let page = html(&report, None);
    assert!(page.contains("2.00 s (partial)"));
    assert!(page.contains("case &lt;one&gt;"));
    assert!(!page.contains("Skipped means explicitly ignored by libtest"));
}

#[test]
fn missing_case_time_and_different_execution_models_are_not_speedups() {
    let old = measured(
        MeasurementKind::Cases,
        success(),
        vec![case_record(Some(5.0), false)],
        vec![],
    );
    let new = measured(
        MeasurementKind::Cases,
        success(),
        vec![case_record(None, false)],
        vec![],
    );
    let page = html(&new, Some(&old));
    assert!(page.contains("incomplete group"));
    assert!(!page.contains("-5.00 s"));
    let cargo = report("", &metadata(), success());
    let page = html(&new, Some(&cargo));
    assert!(page.contains("different execution model"));
    assert!(!page.contains("Previous seconds"));
}

#[test]
fn saved_report_readers_share_version_and_duration_validation() {
    let report = measured(
        MeasurementKind::Cases,
        success(),
        vec![case_record(Some(1.0), false)],
        vec![],
    );
    let mut value = serde_json::to_value(report).unwrap();
    assert_eq!(
        Report::parse(&value.to_string()).unwrap().kind,
        MeasurementKind::Cases
    );
    value["schema"] = json!(1);
    assert!(Report::parse(&value.to_string()).is_err());
    value["schema"] = json!(2);
    value["groups"][0]["seconds"] = json!(-1);
    assert!(Report::parse(&value.to_string()).is_err());
    value["groups"][0]["seconds"] = json!(1);
    value.as_object_mut().unwrap().remove("kind");
    value["groups"][0]
        .as_object_mut()
        .unwrap()
        .remove("unmeasured");
    assert_eq!(
        Report::parse(&value.to_string()).unwrap().kind,
        MeasurementKind::TestPrograms
    );
}

#[test]
fn invalid_command_time_is_unknown_for_every_execution_adapter() {
    for kind in [MeasurementKind::Cases, MeasurementKind::TestPrograms] {
        let mut context = success();
        context.wall_seconds = Some(-1.0);
        let report = measured(kind, context, vec![case_record(Some(1.0), false)], vec![]);
        assert_eq!(report.context.wall_seconds, None);
        assert!(!report.complete);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("Invalid command wall duration"))
        );
    }
}

fn metadata() -> Metadata {
    serde_json::from_value(json!({
        "workspace_root": "/work",
        "workspace_members": ["alpha", "contract", "beta", "kit", "checks"],
        "packages": [
            {"id": "alpha", "name": "alpha", "manifest_path": "/work/modules/tools/alpha/Cargo.toml", "targets": [
                {"name": "alpha", "kind": ["lib"], "src_path": "/work/modules/tools/alpha/src/lib.rs"},
                {"name": "lnk-alpha", "kind": ["bin"], "src_path": "/work/modules/tools/alpha/src/main.rs"},
                {"name": "main", "kind": ["test"], "src_path": "/work/modules/tools/alpha/tests/main.rs"}
            ]},
            {"id": "contract", "name": "alpha-contract", "manifest_path": "/work/modules/tools/alpha/contract/Cargo.toml", "targets": [
                {"name": "alpha_contract", "kind": ["lib"], "src_path": "/work/modules/tools/alpha/contract/src/lib.rs"}
            ]},
            {"id": "beta", "name": "beta", "manifest_path": "/work/adapters/tools/beta/Cargo.toml", "targets": [
                {"name": "lnk-beta", "kind": ["bin"], "src_path": "/work/adapters/tools/beta/src/main.rs"},
                {"name": "main", "kind": ["test"], "src_path": "/work/adapters/tools/beta/tests/main.rs"}
            ]},
            {"id": "kit", "name": "kit", "manifest_path": "/work/packages/kit/Cargo.toml", "targets": [
                {"name": "kit", "kind": ["lib"], "src_path": "/work/packages/kit/src/lib.rs"}
            ]},
            {"id": "checks", "name": "checks", "manifest_path": "/work/tests/checks/Cargo.toml", "targets": [
                {"name": "checks", "kind": ["test"], "src_path": "/work/tests/checks/tests/main.rs"}
            ]}
        ]
    })).unwrap()
}

fn artifact(package: &str, executable: &str, fresh: bool) -> String {
    json!({"reason": "compiler-artifact", "package_id": package, "executable": format!("/work/target/debug/deps/{executable}"), "fresh": fresh}).to_string()
}

fn summary(result: &str, time: &str) -> String {
    let failed = u64::from(result == "FAILED");
    format!(
        "test result: {result}. 3 passed; {failed} failed; 2 ignored; 0 measured; 4 filtered out; finished in {time}"
    )
}

fn success() -> Context {
    Context {
        status: Some(0),
        ..Context::default()
    }
}

#[test]
fn fresh_artifacts_disambiguate_identical_test_names_and_group_contracts() {
    let log = format!(
        "{}\n{}\n{}\nFinished `test` profile in 1m 2.30s\nRunning tests/main.rs (target/debug/deps/main-a1)\n{}\nRunning tests/main.rs (target/debug/deps/main-b2)\n{}\nRunning unittests src/lib.rs (target/debug/deps/alpha_contract-c3)\n{}\n",
        artifact("alpha", "main-a1", true),
        artifact("beta", "main-b2", false),
        artifact("contract", "alpha_contract-c3", true),
        summary("ok", "2.00s"),
        summary("FAILED", "3.00s"),
        summary("ok", "1.00s")
    );
    let report = report(
        &log,
        &metadata(),
        Context {
            status: Some(101),
            ..Context::default()
        },
    );
    assert_eq!(report.programs[0].package.as_deref(), Some("alpha"));
    assert_eq!(report.programs[1].package.as_deref(), Some("beta"));
    assert_eq!(report.groups.len(), 2);
    let alpha = report
        .groups
        .iter()
        .find(|g| g.name.ends_with("/alpha"))
        .unwrap();
    assert_eq!(alpha.seconds, 3.0);
    assert_eq!(alpha.programs, 2);
    assert_eq!(report.compilation_seconds, Some(62.3));
    assert_eq!(report.measured_program_seconds, 6.0);
    assert_eq!(report.context.wall_seconds, None);
    assert!(!report.complete);
    assert_eq!(report.programs[1].failed, 1);
    assert_eq!(report.programs[1].ignored, 2);
    assert_eq!(report.programs[1].filtered, 4);
}

#[test]
fn ambiguous_plain_output_is_unassigned_instead_of_guessed() {
    let log = format!(
        "Running tests/main.rs (target/debug/deps/main-abc)\n{}",
        summary("ok", "1.00s")
    );
    let report = report(&log, &metadata(), success());
    assert!(report.programs[0].package.is_none());
    assert!(!report.complete);
    assert_eq!(report.groups[0].name, "unassigned");
    assert!(report.warnings.iter().any(|w| w.contains("unassigned")));
}

#[test]
fn a_crashed_or_truncated_program_has_unknown_duration() {
    let log = format!(
        "Running unittests src/lib.rs (target/debug/deps/kit-abc)\n{}\nRunning tests/main.rs (target/debug/deps/main-def)\nrunning 4 tests\nerror: test failed",
        summary("ok", "0.01s")
    );
    let report = report(
        &log,
        &metadata(),
        Context {
            status: Some(101),
            ..Context::default()
        },
    );
    assert_eq!(report.programs.len(), 2);
    assert_eq!(report.programs[1].seconds, None);
    assert_eq!(report.programs[1].outcome, None);
    assert!(!report.complete);
    assert_eq!(report.measured_program_seconds, 0.01);
    assert!(report.groups.iter().any(|g| g.incomplete == 1));
}

#[test]
fn successful_summaries_without_exit_status_do_not_claim_completion() {
    let log = format!(
        "Running unittests src/lib.rs (target/debug/deps/kit-abc)\n{}",
        summary("ok", "0.00s")
    );
    let report = report(&log, &metadata(), Context::default());
    assert!(!report.complete);
    assert_eq!(report.measured_program_seconds, 0.0);
    let html = html(&report, None);
    assert!(html.contains("No positive measured durations"));
    assert!(!html.contains("NaN"));
    assert!(!html.contains("conic-gradient"));
}

#[test]
fn doc_tests_and_shared_packages_keep_their_own_groups() {
    let log = format!(
        "Doc-tests kit\n{}\nRunning tests/main.rs (target/debug/deps/checks-abc)\n{}",
        summary("ok", "0.02s"),
        summary("ok", "0.03s")
    );
    let report = report(&log, &metadata(), success());
    assert_eq!(report.programs[0].package.as_deref(), Some("kit"));
    assert_eq!(report.programs[0].group, "shared: packages/kit");
    assert_eq!(report.programs[1].group, "tests: tests/checks");
    assert!(report.complete);
}

#[test]
fn nested_summaries_leave_the_outer_harness_counts_and_outcome() {
    let log = format!(
        "Running tests/main.rs (target/debug/deps/checks-abc)\n{}\n{}\n{}\n",
        summary("ok", "0.01s"),
        summary("FAILED", "0.02s"),
        summary("ok", "0.10s")
    );
    let report = report(&log, &metadata(), success());
    assert!(report.complete, "{:?}", report.warnings);
    assert_eq!(report.programs.len(), 1);
    assert_eq!(report.programs[0].seconds, Some(0.10));
    assert_eq!(report.programs[0].failed, 0);
    assert_eq!(report.programs[0].ignored, 2);
}

#[test]
fn a_child_summary_cannot_make_its_crashed_parent_pass() {
    let log = format!(
        "Running tests/main.rs (target/debug/deps/checks-abc)\n{}\nerror: test failed, to rerun pass --test main\n",
        summary("ok", "0.01s")
    );
    let report = report(
        &log,
        &metadata(),
        Context {
            status: Some(101),
            ..Context::default()
        },
    );
    assert!(!report.complete);
    assert_eq!(report.programs[0].outcome, None);
    assert_eq!(report.programs[0].seconds, None);
    assert_eq!(report.programs[0].passed, 0);
}

#[test]
fn malformed_or_orphaned_summaries_are_visible() {
    let log = format!(
        "{}\nRunning unittests src/lib.rs (target/debug/deps/kit-abc)\n{}",
        summary("ok", "0.01s"),
        summary("ok", "NaNs")
    );
    let report = report(&log, &metadata(), success());
    assert_eq!(report.programs[0].seconds, None);
    assert!(!report.complete);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("without a Cargo"))
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("Unrecognized summary"))
    );
}

#[test]
fn an_empty_or_compile_failed_log_is_not_a_successful_measurement() {
    let report = report("error: could not compile alpha", &metadata(), success());
    assert!(!report.complete);
    assert!(report.programs.is_empty());
    assert!(!report.warnings.is_empty());
}

#[test]
fn report_escapes_untrusted_context_and_names() {
    let log = format!(
        "Running unittests src/lib.rs (target/debug/deps/kit-abc)\n{}",
        summary("ok", "1.00s")
    );
    let report = report(
        &log,
        &metadata(),
        Context {
            machine: Some("<script>alert('x')</script>".into()),
            ..success()
        },
    );
    let page = html(&report, None);
    assert!(!page.contains("<script>"));
    assert!(page.contains("&lt;script&gt;"));
    assert!(page.contains("conic-gradient(#2563eb 0.000000deg 360.000000deg)"));
    let roundtrip: Report = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
    assert_eq!(roundtrip.measured_program_seconds, 1.0);
}

#[test]
fn baseline_reports_added_removed_and_incomplete_groups_without_false_zeroes() {
    let old = report(
        &format!("Doc-tests alpha\n{}", summary("ok", "2.00s")),
        &metadata(),
        Context::default(),
    );
    let new = report(
        &format!("Doc-tests kit\n{}", summary("ok", "1.00s")),
        &metadata(),
        success(),
    );
    let page = html(&new, Some(&old));
    assert!(page.contains("selection changed"));
    assert!(page.contains("At least one measurement is failed"));
    assert!(page.contains("module: modules/tools/alpha"));
    assert!(page.contains("shared: packages/kit"));
}

#[test]
fn unknown_group_duration_is_not_reported_as_a_speedup() {
    let old = report(
        &format!("Doc-tests kit\n{}", summary("ok", "5.00s")),
        &metadata(),
        success(),
    );
    let new = report("Doc-tests kit\nrunning 3 tests", &metadata(), success());
    let page = html(&new, Some(&old));
    assert!(page.contains("incomplete group"));
    assert!(page.contains("unknown"));
    assert!(!page.contains("-5.00 s"));
}

#[test]
fn empty_duration_is_unknown_rather_than_zero() {
    let log = format!(
        "Running unittests src/lib.rs (target/debug/deps/kit-abc)\n{}",
        summary("ok", "")
    );
    let report = report(&log, &metadata(), success());
    assert_eq!(report.programs[0].seconds, None);
    assert!(!report.complete);
}

#[test]
fn a_successful_custom_harness_leaves_timing_unknown_without_claiming_cargo_failed() {
    let log = format!(
        "{}\nRunning tests/main.rs (target/debug/deps/main-abc)\ncustom protocol checks passed\n",
        artifact("alpha", "main-abc", true)
    );
    let report = report(&log, &metadata(), success());
    assert_eq!(report.programs[0].package.as_deref(), Some("alpha"));
    assert_eq!(report.programs[0].seconds, None);
    assert_eq!(report.groups[0].incomplete, 1);
    assert!(!report.complete);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("nonconforming harness"))
    );
    let page = html(&report, None);
    assert!(page.contains("Cargo succeeded, but timing coverage is incomplete"));
    assert!(!page.contains("Cargo failed."));
    assert!(!page.contains("Cargo exit status is unknown."));
}

#[test]
fn ranked_table_precedes_optional_chart_and_shows_observed_counts_in_order() {
    let log = format!(
        "{}\nRunning tests/main.rs (target/debug/deps/main-abc)\n{}\nRunning unittests src/lib.rs (target/debug/deps/alpha_contract-def)\n{}\nDoc-tests kit\n{}",
        artifact("alpha", "main-abc", true),
        summary("ok", "2.00s"),
        summary("FAILED", "2.00s"),
        summary("ok", "12.00s")
    );
    let report = report(
        &log,
        &metadata(),
        Context {
            status: Some(101),
            ..Context::default()
        },
    );
    let alpha = report
        .groups
        .iter()
        .find(|g| g.name.ends_with("/alpha"))
        .unwrap();
    assert_eq!(
        (alpha.seconds, alpha.passed, alpha.ignored, alpha.failed),
        (4.0, 6, 4, 1)
    );
    let page = html(&report, None);
    let headers = "<th>Area</th><th class=\"num\">Measured time</th><th class=\"num\">Share</th><th class=\"num\">Passed</th><th class=\"num\">Skipped</th><th class=\"num\">Failed</th>";
    assert!(page.contains(headers));
    assert!(page.find(headers).unwrap() < page.find("<details>").unwrap());
    assert!(page.contains("<td class=\"num\">4.00 s</td><td class=\"num\">25.0%</td><td class=\"num\">6</td><td class=\"num\">4</td><td class=\"num\">1</td>"));
    assert!(page.contains("Skipped means explicitly ignored by libtest"));
    assert!(page.contains("gate coverage is unknown"));
}

#[test]
fn older_baseline_groups_without_passed_counts_remain_readable() {
    let report = report(
        &format!("Doc-tests kit\n{}", summary("ok", "1.00s")),
        &metadata(),
        success(),
    );
    let mut saved = serde_json::to_value(&report).unwrap();
    saved["groups"][0].as_object_mut().unwrap().remove("passed");
    saved["groups"][0]
        .as_object_mut()
        .unwrap()
        .remove("skipped");
    saved["context"]
        .as_object_mut()
        .unwrap()
        .remove("filtered_skip_reason");
    let old: Report = serde_json::from_value(saved).unwrap();
    assert_eq!(old.groups[0].passed, 0);
    assert_eq!(old.groups[0].skipped, 0);
    assert_eq!(old.context.filtered_skip_reason, None);
    assert!(html(&report, Some(&old)).contains("Previous measurement"));
}

#[test]
fn filtered_tests_are_environment_skips_only_with_an_explicit_reason() {
    let log = format!("Doc-tests kit\n{}", summary("FAILED", "1.00s"));
    let regular = report(
        &log,
        &metadata(),
        Context {
            status: Some(101),
            ..Context::default()
        },
    );
    let classified = report(
        &log,
        &metadata(),
        Context {
            status: Some(101),
            filtered_skip_reason: Some(
                "Verified root-only tests excluded for the current user".into(),
            ),
            ..Context::default()
        },
    );
    assert_eq!(regular.groups[0].skipped, 2);
    assert_eq!(classified.groups[0].skipped, 6);
    for report in [&regular, &classified] {
        assert_eq!(report.groups[0].passed, 3);
        assert_eq!(report.groups[0].failed, 1);
        assert_eq!(report.groups[0].ignored, 2);
        assert_eq!(report.programs[0].ignored, 2);
        assert_eq!(report.programs[0].filtered, 4);
        assert!(!report.complete);
    }
    let page = html(&classified, None);
    assert!(page.contains("explicitly classified as environment skips"));
    assert!(page.contains("Verified root-only tests excluded for the current user"));
    assert!(page.contains("Passed and failed counts are unchanged"));
}

#[test]
fn malformed_trailing_counter_commits_no_outcomes() {
    let log = "Doc-tests kit\ntest result: ok. 8 passed; 0 failed; 2 ignored; 0 measured; 3 broken; finished in 1.00s";
    let report = report(log, &metadata(), success());
    let program = &report.programs[0];
    assert_eq!(
        (
            program.passed,
            program.failed,
            program.ignored,
            program.filtered
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(program.seconds, None);
    assert_eq!(program.outcome, None);
    let page = html(&report, None);
    assert!(page.contains("<td class=\"num\">unknown</td><td class=\"num\">unknown</td><td class=\"num\">unknown</td><td class=\"num\">unknown</td><td class=\"num\">unknown</td>"));
    assert!(!report.complete);
}

#[test]
fn mixed_coverage_marks_observed_counts_partial() {
    let log = format!(
        "Doc-tests kit\n{}\nDoc-tests kit\nprogram exited without a summary",
        summary("ok", "1.00s")
    );
    let report = report(&log, &metadata(), success());
    assert_eq!(report.groups[0].passed, 3);
    assert_eq!(report.groups[0].incomplete, 1);
    let page = html(&report, None);
    assert!(page.contains("<td class=\"num\">3 (partial)</td><td class=\"num\">2 (partial)</td><td class=\"num\">0 (partial)</td>"));
}

#[test]
fn phase_report_keeps_work_elapsed_missing_costs_and_failures_distinct() {
    let mut report = report("", &metadata(), Context::default());
    report.validation = Some(PhaseMeasurements::parse(r#"{"schema":1,"elapsed_seconds":150,"phases":[{"name":"clippy","wall_seconds":120,"status":0},{"name":"bin-build","wall_seconds":90,"status":101},{"name":"test-command"}]}"#).unwrap());
    let page = html(&report, None);
    assert!(page.contains("<dt>Overall validation elapsed</dt><dd>150.00 s</dd>"));
    assert!(
        page.contains("<dt>Summed measured phase work</dt><dd>210.00 s (partial coverage)</dd>")
    );
    assert!(page.contains("unknown (not recorded)"));
    assert!(page.contains(
        "<td>test-command</td><td class=\"num\">unknown</td><td>unknown / not recorded</td>"
    ));
    assert!(page.contains("failed (101)"));
    assert!(page.contains("At least one validation phase failed"));
    assert!(page.find("<h2>Validation phases").unwrap() < page.find("<h2>Ranked areas").unwrap());
    let saved: Report = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
    assert_eq!(saved.validation.unwrap().summed_seconds(), Some(210.0));
}

#[test]
fn old_reports_do_not_invent_validation_phase_costs() {
    let report = report("", &metadata(), Context::default());
    let mut saved = serde_json::to_value(&report).unwrap();
    saved.as_object_mut().unwrap().remove("validation");
    let old: Report = serde_json::from_value(saved).unwrap();
    assert!(old.validation.is_none());
    assert!(html(&old, None).contains("Validation phases were not captured"));
}

/// A run where alpha's program fails three tests: one the change broke,
/// one a missing program broke and one flaky; beta's fails one that the
/// base's run failed too.
fn failing_run() -> String {
    format!(
        "{}\n{}\nRunning tests/main.rs (target/debug/deps/main-a1)\n\
         test mine ... FAILED\ntest needs_programs ... FAILED\ntest flaky::one ... FAILED\n\
         test fine ... ok\n\nfailures:\n\n---- mine stdout ----\n\n\
         thread 'mine' (7) panicked at tests/main.rs:3:5:\nleft was 1, right 2\nstack backtrace:\n\
         ---- needs_programs stdout ----\n\
         thread 'needs_programs' (8) panicked at tests/main.rs:9:5:\n\
         /work/target/debug/lnk-x isn't built: the programs a test runs are built first (cargo build --workspace --bins)\n\
         ---- flaky::one stdout ----\n\
         thread 'flaky::one' (9) panicked at tests/main.rs:12:5:\nwaited 30s\n\n\
         {}\nRunning tests/main.rs (target/debug/deps/main-b2)\ntest old ... FAILED\n{}\n",
        artifact("alpha", "main-a1", true),
        artifact("beta", "main-b2", true),
        summary("FAILED", "1.00s"),
        summary("FAILED", "1.00s"),
    )
}

#[test]
fn each_failing_test_is_named_with_what_it_said() {
    let run = report(&failing_run(), &metadata(), success());
    let alpha = &run.programs[0].failures;
    assert_eq!(
        alpha,
        &[
            Failure {
                test: "mine".into(),
                said: Some("left was 1, right 2".into())
            },
            Failure {
                test: "needs_programs".into(),
                said: Some("/work/target/debug/lnk-x isn't built: the programs a test runs are built first (cargo build --workspace --bins)".into())
            },
            Failure {
                test: "flaky::one".into(),
                said: Some("waited 30s".into())
            },
        ]
    );
    assert_eq!(run.programs[1].failures[0].said, None);
}

#[test]
fn failures_are_sorted_as_the_machines_a_flakes_the_bases_or_yours() {
    use check::{Flake, Whose, signatures, sort, text};
    let run = report(&failing_run(), &metadata(), success());
    let base_log = format!(
        "{}\nRunning tests/main.rs (target/debug/deps/main-b2)\ntest old ... FAILED\n{}\n",
        artifact("beta", "main-b2", true),
        summary("FAILED", "1.00s")
    );
    let base = report(&base_log, &metadata(), success());
    let flakes = [Flake {
        package: "alpha".into(),
        test: "flaky::one".into(),
    }];
    let signatures = signatures();
    let sorted = sort(&run, Some(&base), &flakes, &signatures);
    let whose: Vec<_> = sorted
        .iter()
        .map(|s| (s.test.as_str(), s.whose.clone()))
        .collect();
    assert_eq!(
        whose,
        [
            ("mine", Whose::Yours),
            (
                "needs_programs",
                Whose::Machine("programs not built".into())
            ),
            ("flaky::one", Whose::Flake),
            ("old", Whose::Base),
        ]
    );
    let said = text(&sorted, &signatures);
    assert!(
        said.starts_with("4 failed: 1 yours, 1 the base's, 1 known flakes, 1 the machine's"),
        "{said}"
    );
    assert!(said.contains("  programs not built (1 tests): "), "{said}");
    // Without the base's run, its failure is nobody else's.
    let alone = sort(&run, None, &flakes, &signatures);
    assert_eq!(alone[3].whose, Whose::Yours);
}

#[test]
fn a_flakes_list_is_read_for_its_tests() {
    let listed = check::flakes(
        "[[flake]]\npackage = \"alpha\"\ntest = \"flaky::one\"\nseen = \"2026-11-17\"\n",
    )
    .unwrap();
    assert_eq!(
        listed,
        [check::Flake {
            package: "alpha".into(),
            test: "flaky::one".into()
        }]
    );
    assert!(check::flakes("[[flake]]\npackage = 1\n").is_err());
    let read = include_str!("../../repository/flakes.toml");
    assert!(!check::flakes(read).unwrap().is_empty());
}
