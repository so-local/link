//! A module's tool actions rendered as its template, then filled as its
//! program would at runtime.

link_test_kit::program!(unit);

use std::collections::BTreeSet;
use std::path::Path;

use link_actions::tools::{Fill, Fills, SUMMARY_META, offer};

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/fixtures/declarations/module")
}

/// The example's tool action as `tools/list` gives it: its declared
/// name, its fills in place, its title's arguments in `_meta`, and no
/// `x-link-*` keyword for a model's provider to see; left out while its
/// condition doesn't hold.
#[test]
fn a_tool_action_is_offered_as_declared_and_filled() {
    let template = link_actions_build::tools(&fixture()).unwrap();
    let fills = |conditions: &[&'static str]| Fills {
        values: [
            ("deepest", Fill::Text("9".into())),
            (
                "places",
                Fill::Words(vec!["kitchen".into(), "garage".into()]),
            ),
        ]
        .into(),
        conditions: conditions.iter().copied().collect::<BTreeSet<_>>(),
    };
    let offered = offer(&template, &fills(&["inside"])).unwrap();
    let calls = offered.as_array().unwrap();
    assert_eq!(calls.len(), 1, "{offered:#}");
    let inspect = &calls[0];
    assert_eq!(inspect["name"], "inspect");
    assert_eq!(
        inspect["description"],
        "Look inside the environment, at most 9 deep"
    );
    // Each hint as declared, and none it leaves out.
    assert_eq!(
        inspect["annotations"],
        serde_json::json!({"readOnlyHint": true})
    );
    let place = &inspect["inputSchema"]["properties"]["place"];
    assert_eq!(place["enum"], serde_json::json!(["kitchen", "garage"]));
    assert_eq!(
        inspect["_meta"][SUMMARY_META],
        serde_json::json!(["place", "depth"])
    );
    assert!(
        !inspect["inputSchema"].to_string().contains("x-link-"),
        "{inspect:#}"
    );
    assert!(
        offer(&template, &fills(&[]))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}
