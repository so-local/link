use super::*;

fn agent() -> Unit {
    Unit {
        id: "a-1234".into(),
        kind: "agent".into(),
        parent: Some("m-99".into()),
        name: Some("main".into()),
        machine: Some("m-99".into()),
        owner: Some("me@laptop".into()),
        teams: vec!["t-1".into(), "t-2".into()],
    }
}

#[test]
fn every_name_ends_with_how_it_rolls_up() {
    for m in MEASURES {
        assert!(m.name.starts_with("link."), "{}", m.name);
        assert!(m.name.ends_with(m.rollup.suffix()), "{}", m.name);
        assert_eq!(find(m.name), Some(m));
    }
}

#[test]
fn otlp_reads_back_as_written() {
    let reports = vec![
        // A machine may only place its child, with no measures of its own.
        Report {
            unit: Unit {
                id: "m-99".into(),
                kind: "machine".into(),
                ..Unit::default()
            },
            points: Vec::new(),
        },
        Report {
            unit: agent(),
            points: vec![
                Point::new("link.memory.used.sum", 12_345_678.0, 5),
                Point::new("link.cpu.used.sum", 1.5, 6).since(1),
                Point::new("link.time.to_model.own", 0.25, 7),
                Point::new("link.tokens.input.sum", 900.0, 7).since(6),
            ],
        },
    ];
    let json = otlp(&reports, "lnk-measure");
    assert_eq!(read_otlp(&json), reports);
    let text = json.to_string();
    assert!(text.contains(r#""aggregationTemporality":2"#), "{text}");
    assert!(
        text.contains(
            r#""key":"link.team.ids","value":{"arrayValue":{"values":[{"stringValue":"t-1"}"#
        ),
        "{text}"
    );
    assert!(text.contains(r#""asInt":"12345678""#), "{text}");
}

/// Many reports of one unit, as a batch of requests is, are one
/// resource with each measure described once.
#[test]
fn a_units_reports_are_one_resource() {
    let one = |at: u64| Report {
        unit: agent(),
        points: vec![
            Point::new("link.tokens.input.sum", 900.0, at).since(at - 1),
            Point::new("link.cost.sum", 12.0, at).since(at - 1),
        ],
    };
    let reports: Vec<Report> = (2..1_002).map(one).collect();
    let json = otlp(&reports, "lnk-measure");
    assert_eq!(json["resourceMetrics"].as_array().unwrap().len(), 1);
    let text = json.to_string();
    assert_eq!(text.matches("\"description\"").count(), 2, "{text:.300}");
    // Every value still reads back, as one report of the unit.
    let read = read_otlp(&json);
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].points.len(), 2_000);
    // Under half the size of a resource a report: it names the unit
    // and describes each measure once, not a thousand times.
    let apart: usize = reports
        .iter()
        .map(|r| {
            otlp(std::slice::from_ref(r), "lnk-measure")
                .to_string()
                .len()
        })
        .sum();
    assert!(text.len() * 2 < apart, "{} against {apart}", text.len());
}

/// Whatever a program sends, only numbers of Link's measures and the
/// unit's plain names come through.
#[test]
fn reading_drops_everything_the_schema_lacks() {
    let secret = "the purple walrus";
    let json = json!({"resourceMetrics": [{
        "resource": {"attributes": [
            {"key": ID, "value": {"stringValue": secret}},
            {"key": "prompt", "value": {"stringValue": secret}},
        ]},
        "scopeMetrics": [{"scope": {"name": secret}, "metrics": [
            {"name": secret, "gauge": {"dataPoints": [{"asInt": "1", "timeUnixNano": "1"}]}},
            {"name": "link.turns.sum", "description": secret, "sum": {"dataPoints": [
                {"asInt": "1", "timeUnixNano": "1",
                 "attributes": [{"key": "text", "value": {"stringValue": secret}}]},
                {"asDouble": -1.0, "timeUnixNano": "1"},
                {"asDouble": secret, "timeUnixNano": "1"},
            ]}},
        ]}],
    }]});
    let reports = read_otlp(&json);
    assert_eq!(reports.len(), 1);
    assert!(!reports[0].unit.valid());
    assert_eq!(reports[0].points.len(), 1);
    let written = otlp(
        &[Report {
            unit: agent(),
            points: reports[0].points.clone(),
        }],
        "test",
    );
    assert!(!written.to_string().contains(secret));
}

/// An agent's teams are its IDs alone: one with text where an ID
/// goes makes a unit the schema refuses.
#[test]
fn teams_are_ids_only() {
    let mut unit = agent();
    assert!(unit.valid());
    unit.teams.push("the purple walrus".into());
    assert!(!unit.valid());
}

/// Only the harness module says how long a start or a stop took,
/// each one a value of its own.
#[test]
fn starts_and_stops_are_the_harness_modules_to_say() {
    let module: Vec<&str> = MEASURES
        .iter()
        .filter(|m| m.by == By::Module)
        .map(|m| m.name)
        .collect();
    assert_eq!(
        module,
        [
            "link.time.start_cold.own",
            "link.time.start_warm.own",
            "link.time.stop.own"
        ]
    );
    assert!(
        MEASURES
            .iter()
            .all(|m| m.by != By::Module || m.kind == Kind::Each)
    );
    // A harness says only what it counts itself.
    assert!(
        MEASURES
            .iter()
            .all(|m| m.by != By::Harness || matches!(m.kind, Kind::Delta | Kind::Each))
    );
}

/// Only a relay says what it couldn't hand on, a running total since
/// it started, as its unit's own.
#[test]
fn undelivered_hooks_are_the_relays_to_say() {
    let relay: Vec<&Measure> = MEASURES.iter().filter(|m| m.by == By::Relay).collect();
    assert_eq!(
        relay.iter().map(|m| m.name).collect::<Vec<_>>(),
        [
            "link.hooks.undelivered.sum",
            "link.relay.connections.sum",
            "link.relay.requests.sum",
            "link.relay.refusals.sum",
            "link.relay.failures.sum",
            "link.relay.unadmitted.sum",
            "link.relay.bytes.sum"
        ]
    );
    assert!(
        relay
            .iter()
            .all(|m| matches!(m.kind, Kind::Total | Kind::Gauge))
    );
    let unit = Unit {
        id: "hooks.example.com".into(),
        kind: "relay".into(),
        name: Some("hooks".into()),
        ..Unit::default()
    };
    assert!(unit.valid());
    let json = otlp(
        &[Report {
            unit: unit.clone(),
            points: vec![Point::new("link.hooks.undelivered.sum", 3.0, 9).since(2)],
        }],
        "link-hooks",
    );
    let metric = &json["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0];
    assert_eq!(metric["sum"]["aggregationTemporality"], 2);
    assert_eq!(metric["sum"]["dataPoints"][0]["asInt"], "3");
    let read = read_otlp(&json);
    assert_eq!(read[0].unit, unit);
    assert_eq!(read[0].points[0].value, 3.0);
    assert_eq!(read[0].points[0].start, Some(2));
}

#[test]
fn the_metrics_path_is_added_once() {
    assert_eq!(
        metrics_url("http://localhost:4318"),
        "http://localhost:4318/v1/metrics"
    );
    assert_eq!(
        metrics_url("https://c:4318/v1/metrics/"),
        "https://c:4318/v1/metrics"
    );
}

#[test]
fn plain_is_ids_and_names_only() {
    assert!(plain("main"));
    assert!(plain("0d3c-ab/4123"));
    assert!(!plain("two words"));
    assert!(!plain(""));
    assert!(!plain(&"a".repeat(65)));
}

/// A process is named by a program Link knows, or not at all: never
/// by what it named itself.
#[test]
fn a_process_is_named_only_by_a_known_program() {
    assert_eq!(program("bwrap"), Some("bwrap"));
    assert_eq!(program("lnk-bucket-stor"), Some("lnk-bucket-store"));
    assert_eq!(program("purple-walrus"), None);
    assert_eq!(program("lnk-link"), None);
    let process = |name: &str| Unit {
        id: "a-1234/42".into(),
        kind: "process".into(),
        name: Some(name.into()),
        ..Unit::default()
    };
    assert!(process("node").valid());
    assert!(!process("purple-walrus").valid());
    assert!(agent().valid());
}

#[test]
fn sums_are_never_infinite() {
    assert_eq!(add(1e308, 1e308), f64::MAX);
    assert_eq!(add(1.0, 2.0), 3.0);
    let json = otlp(
        &[Report {
            unit: agent(),
            points: vec![Point::new("link.cost.sum", add(1e308, 1e308), 1)],
        }],
        "test",
    );
    assert_eq!(read_otlp(&json)[0].points.len(), 1);
}

/// Every program Link releases (`lnk`, and each module's `lnk-<id>`) is on
/// the list, and no two of the list's are one name
/// once Linux cuts it to 15 bytes.
#[test]
fn every_program_link_releases_is_named() {
    let output = link_test_kit::cargo()
        .args([
            "metadata",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let released = metadata["packages"].as_array().unwrap().iter().filter(|p| {
        p["name"] == "link-cli"
            || (p["metadata"]["link"]["role"] != "fixture"
                && p["targets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["name"].as_str().unwrap().starts_with("lnk-")))
    });
    let mut checked = 0;
    for package in released {
        for target in package["targets"].as_array().unwrap() {
            let bin = target["kind"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| k == "bin");
            let name = target["name"].as_str().unwrap();
            if bin {
                assert!(PROGRAMS.contains(&name), "{name} isn't on PROGRAMS");
                checked += 1;
            }
        }
    }
    assert!(checked > 10, "only {checked} programs found");
    for name in PROGRAMS {
        let cut = &name[..name.len().min(15)];
        assert_eq!(program(cut), Some(*name), "{cut}");
    }
}
