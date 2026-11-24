//! `lnk box list --json` writes what its declared output reads: each box
//! the hand-written `Listed` serializes is a `listed` of `types.toml`.

use link_actions::value::Validate;
use link_box::cli::Listed as Declared;
use link_box_contract::Listed;
use link_cloud_contract::prices::Price;

/// Reads `listed` as the declaration's type, closed to unknown keys,
/// and checks its declared bounds.
fn declared(listed: &Listed) -> Declared {
    let json = serde_json::to_value(listed).unwrap();
    let read: Declared = serde_json::from_value(json.clone())
        .unwrap_or_else(|err| panic!("{json} is no declared listed: {err}"));
    read.validate("").unwrap();
    assert_eq!(serde_json::to_value(&read).unwrap(), json);
    read
}

#[test]
fn a_bare_box_is_its_declared_output() {
    declared(&Listed {
        name: "aws-1".into(),
        ..Default::default()
    });
}

#[test]
fn a_known_machine_price_and_deadline_are_declared() {
    let read = declared(&Listed {
        name: "aws-1".into(),
        account: "aws".into(),
        kind: "aws".into(),
        place: "us-east-1".into(),
        id: "i-0123".into(),
        machine_id: "a1b2c3d4e5f6".into(),
        size: "t3.small".into(),
        ip: Some("203.0.113.7".into()),
        state: "running".into(),
        created: 1_700_000_000,
        found: "here".into(),
        stopped_by_cloud: true,
        tags: [("link".to_string(), "box".to_string())].into(),
        price: Some(Price {
            machine: Some(0.02),
            disk: Some(0.001),
            ..Default::default()
        }),
        deadline: Some(1_700_003_600),
    });
    assert!(read.resource.is_some());
}

#[test]
fn an_unknown_machine_id_leaves_resource_out() {
    let read = declared(&Listed {
        name: "gcp-1".into(),
        machine_id: "not an id".into(),
        ..Default::default()
    });
    assert_eq!(read.resource, None);
}
