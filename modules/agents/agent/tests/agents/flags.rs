//! A start's field flags and `lnk agent spec show`.

use super::*;

/// A start's field flags are the lines of its spec: laid over its
/// `--spec` file, or over the agent's own spec without one, kept, and
/// printed back by `lnk agent spec show`.
#[test]
fn a_starts_field_flags_are_lines_of_its_spec() {
    let world = World::new("field-flags");
    let given = world.home.join("given.toml");
    std::fs::write(&given, "schema = 1\n[turns]\nrounds = 7\n").unwrap();
    let given = given.display().to_string();
    ok(&world.start_on_owned_port(
        "flagged",
        &[
            "create",
            "flagged",
            "--harness",
            "fake",
            "--spec",
            &given,
            "--turns.calls",
            "5",
        ],
    ));
    let printed = ok(&world.lnk(&["spec", "show", "flagged"]));
    assert!(printed.contains("rounds = 7"), "{printed}");
    assert!(printed.contains("calls = 5"), "{printed}");
    // Without a file, the flags edit the agent's own spec, keeping the rest.
    ok(&world.start_on_owned_port("flagged", &["start", "-a", "flagged", "--turns.calls", "6"]));
    let printed = ok(&world.lnk(&["spec", "show", "flagged"]));
    assert!(printed.contains("rounds = 7"), "{printed}");
    assert!(printed.contains("calls = 6"), "{printed}");
    // A file alone, and flags alone, print the spec they make.
    let alone = ok(&world.lnk(&["spec", "show", "--spec", &given, "--turns.calls", "3"]));
    assert!(
        alone.contains("rounds = 7") && alone.contains("calls = 3"),
        "{alone}"
    );
    // An agent and a file are two specs: naming both is refused before
    // either is read.
    let both = world.lnk(&["spec", "show", "flagged", "--spec", &given]);
    assert_eq!(both.status.code(), Some(2), "{both:?}");
    // JSON is the spec itself, what a file holds, whose each key is
    // left to the text.
    let json: serde_json::Value =
        serde_json::from_str(&ok(&world.lnk(&["spec", "show", "flagged", "--json"]))).unwrap();
    assert_eq!(json["turns"]["calls"], 6, "{json}");
    assert!(
        json.get("from").is_none() && json.get("spec").is_none(),
        "{json}"
    );
    // A key the shape lacks, and a flag start lacks, are refused.
    for args in [
        &["start", "-a", "flagged", "--turns.colour", "red"][..],
        &["start", "-a", "flagged", "--model", "local/qwen3"],
    ] {
        let out = world.lnk(args);
        assert!(!out.status.success(), "{args:?}");
    }
}
