//! The runtime resolves requirements from these fixtures.

link_test_kit::program!(unit);

use link_component::requirements::{Environment, Operation, Values, required};
use link_component::{Component, Kind, Resolved, Source, Value};
use std::collections::BTreeMap;

fn registry() -> BTreeMap<String, Resolved> {
    [
        ("worker", include_str!("fixtures/worker.json")),
        ("guard", include_str!("fixtures/guard.json")),
    ]
    .into_iter()
    .map(|(id, json)| {
        let source: Source = serde_json::from_str(json).unwrap();
        let kind = if id == "worker" {
            Kind::Harness
        } else {
            Kind::Module
        };
        let component = Component {
            id: id.into(),
            cargo_name: format!("link-{id}"),
            program: format!("lnk-{id}"),
            kind,
            reads_control: true,
            version: "1.0.0".into(),
            license: "MIT".into(),
            description: id.into(),
        };
        (id.into(), source.resolve(component, vec![]).unwrap())
    })
    .collect()
}

#[test]
fn discovery_and_install_do_not_activate_an_engines_operational_requirements() {
    let registry = registry();
    let linux = Environment {
        os: "linux",
        arch: "x86_64",
        libc: "gnu",
    };
    let install = required(
        &registry,
        "worker",
        Some("install"),
        &BTreeMap::new(),
        &linux,
    )
    .unwrap();
    assert_eq!(install.modules, ["worker"]);
    assert!(install.requirements.is_empty());
    let values = BTreeMap::from([("mode".into(), Value::String("restricted".into()))]);
    let operation = required(&registry, "worker", Some("health"), &values, &linux).unwrap();
    assert!(operation.modules.contains(&"guard"));
    assert!(
        operation
            .requirements
            .iter()
            .any(|(_, _, target)| target.id() == "example-engine")
    );
    assert!(
        operation
            .requirements
            .iter()
            .any(|(_, _, target)| target.id() == "linux.user-namespaces")
    );
    let mac = Environment {
        os: "macos",
        arch: "aarch64",
        libc: "none",
    };
    let operation = required(&registry, "worker", Some("health"), &values, &mac).unwrap();
    assert!(
        !operation
            .requirements
            .iter()
            .any(|(_, _, target)| target.id().starts_with("linux."))
    );
    assert!(
        required(
            &registry,
            "worker",
            Some("health"),
            &BTreeMap::from([("mode".into(), Value::Boolean(true))]),
            &linux
        )
        .is_err()
    );
}

#[test]
fn command_selection_preserves_global_flags_defaults_and_option_values() {
    let mut registry = registry();
    let descriptor = registry.get_mut("worker").unwrap();
    let command=serde_json::from_value(serde_json::json!({
        "help":"Start a worker", "options":{
            "agent":{"type":"string","short":"a","long":"agent"},
            "mode":{"type":"string","long":"mode","values":["restricted","open"],"default":"restricted"},
            "verbose":{"type":"boolean","short":"v","default":false}
        }
    })).unwrap();
    descriptor.commands.insert("worker start".into(), command);
    let bound = |args: &[String]| match link_component::requirements::operation_for_args(
        descriptor, args,
    ) {
        Operation::Matched {
            name,
            values: Values::Bound(values),
        } => (name.to_string(), values),
        other => panic!("{args:?} bound nothing: {other:?}"),
    };
    let args = ["worker", "-a", "start", "-v", "start"].map(String::from);
    let (operation, values) = bound(&args);
    assert_eq!(operation, "worker start");
    assert_eq!(values["agent"], Value::String("start".into()));
    assert_eq!(values["mode"], Value::String("restricted".into()));
    assert_eq!(values["verbose"], Value::Boolean(true));
    let args = ["worker", "start", "--mode=open"].map(String::from);
    let (_, values) = bound(&args);
    assert_eq!(values["mode"], Value::String("open".into()));
    let args = ["worker", "--agent", "unknown", "missing"].map(String::from);
    assert_eq!(
        link_component::requirements::operation_for_args(descriptor, &args),
        Operation::Unmatched
    );
}

/// A value its option's type or values refuse binds nothing: the owning
/// parser refuses the args, so selection says so rather than failing.
#[test]
fn a_value_its_option_refuses_binds_nothing() {
    let mut registry = registry();
    let descriptor = registry.get_mut("worker").unwrap();
    let command = serde_json::from_value(serde_json::json!({
        "help": "Start a worker", "options": {
            "all": {"type": "boolean", "long": "all"},
            "count": {"type": "integer", "long": "count"},
            "mode": {"type": "string", "long": "mode", "values": ["restricted", "open"]}
        }
    }))
    .unwrap();
    descriptor.commands.insert("worker start".into(), command);
    for (given, option) in [
        ("--all=qa-invalid", "all"),
        ("--count=many", "count"),
        ("--mode=wide", "mode"),
    ] {
        let args = ["worker", "start", given].map(String::from);
        assert_eq!(
            link_component::requirements::operation_for_args(descriptor, &args),
            Operation::Matched {
                name: "worker start",
                values: Values::Refused {
                    option: option.into()
                },
            },
            "{given}"
        );
    }
}

#[test]
fn a_split_component_is_added_during_upgrade_without_a_required_cycle() {
    let mut registry = registry();
    registry.get_mut("guard").unwrap().upgrade_requires = vec!["worker".into()];
    registry
        .get_mut("worker")
        .unwrap()
        .requires
        .push(serde_json::from_value(serde_json::json!({"dependency":"guard"})).unwrap());
    assert_eq!(
        link_component::required_programs(&registry, &["guard"]).unwrap(),
        ["guard"]
    );
    assert_eq!(
        link_component::upgrade_programs(&registry, &["guard"]).unwrap(),
        ["guard", "worker"]
    );
}
