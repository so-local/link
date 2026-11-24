use super::*;

fn kind() -> link_spec::Exported {
    link_spec::Exported {
        kind: "example".into(),
        schema: 1,
        oldest: 1,
        command: Some("lnk example spec show".into()),
        about: Some("An example.".into()),
        steps: vec![],
        before: Default::default(),
        shape: json!({"type":"object","properties": {
            "name":{"type":"string","description":"Its name."},
            "nested":{"$ref":"#/$defs/Nested"},
            "mode":{"oneOf":[
                {"type":"object","properties":{"kind":{"const":"one"},"added":{"type":"integer","default":3}}},
                {"type":"object","properties":{"kind":{"const":"two"},"other":{"type":"string"}}}
            ]},
            "records":{"type":"array","items":{"$ref":"#/$defs/Nested"}},
            "map":{"type":"object","additionalProperties":{"$ref":"#/$defs/Nested"}}
        },"$defs":{"Nested":{"type":"object","properties":{"value":{"type":"boolean","default":false}}}}}),
    }
}

fn files(source: &str) -> BTreeMap<PathBuf, String> {
    BTreeMap::from([
        (
            "link-public/example/Cargo.toml".into(),
            "[package]\nname='example'\n".into(),
        ),
        ("link-public/example/src/lib.rs".into(), source.into()),
        (
            "link-public/example/src/schema.toml".into(),
            "schema=1\nkind='example'\ncurrent=1\ncommand='lnk example spec show'\nabout='An example.'\n"
                .into(),
        ),
    ])
}

const REGISTRATION: &str = r#"
link_spec::specification!(Example, include_str!("schema.toml"), check = checking::check);
mod checking {
    fn check(value: &Example) { threshold(value); }
    fn threshold(value: &Example) { assert!(value.name.len() < 20); }
}
"#;

#[test]
fn every_nested_variant_map_and_record_field_comes_from_the_typed_shape() {
    let found = discover(&files(REGISTRATION), &[kind()]);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let paths: BTreeSet<_> = found
        .fields
        .iter()
        .map(|field| field.path.as_str())
        .collect();
    for wanted in [
        "name",
        "nested.value",
        "mode.added",
        "mode.other",
        "records.value",
        "map.*.value",
    ] {
        assert!(paths.contains(wanted), "{wanted}: {paths:?}");
    }
    assert!(
        found
            .fields
            .iter()
            .all(|field| field.owner == "link-public/example::Example")
    );
    let nested = found
        .fields
        .iter()
        .find(|field| field.path == "nested.value")
        .unwrap();
    assert_eq!(nested.content["nodes"][0]["default"], false);
}

#[test]
fn moving_lines_and_unrelated_functions_preserve_reviewed_field_content() {
    let before = discover(&files(REGISTRATION), &[kind()]);
    let after = discover(
        &files(&format!(
            "\n\nfn unrelated() {{ let x = 3; }}\n{REGISTRATION}"
        )),
        &[kind()],
    );
    assert!(after.problems.is_empty(), "{:?}", after.problems);
    assert_eq!(before.fields[0].content, after.fields[0].content);
    let changed = discover(&files(&REGISTRATION.replace("< 20", "< 10")), &[kind()]);
    assert_ne!(before.fields[0].content, changed.fields[0].content);
}

#[test]
fn defaults_and_variant_fields_reopen_only_their_relevant_schema_contract() {
    let before = discover(&files(REGISTRATION), &[kind()]);
    let mut changed = kind();
    changed.shape["$defs"]["Nested"]["properties"]["value"]["default"] = true.into();
    let after = discover(&files(REGISTRATION), &[changed]);
    let field = |fields: Vec<SpecField>, path: &str| {
        fields
            .into_iter()
            .find(|field| field.path == path)
            .unwrap()
            .content
    };
    assert_eq!(
        field(before.fields.clone(), "name"),
        field(after.fields.clone(), "name")
    );
    assert_ne!(
        field(before.fields, "nested.value"),
        field(after.fields, "nested.value")
    );
}

#[test]
fn unsupported_or_missing_sources_never_become_an_empty_success() {
    for source in [
        "link_spec::specification!(Example, dynamic_schema());",
        "link_spec::specification!(Example, include_str!(\"missing.toml\"));",
        "link_spec::specification!(Example, include_str!(\"schema.toml\"), future = 1);",
    ] {
        assert!(
            !discover(&files(source), &[kind()]).problems.is_empty(),
            "{source}"
        );
    }
    assert!(!discover(&files(REGISTRATION), &[]).problems.is_empty());
    assert!(
        !discover(&files("struct Example;"), &[kind()])
            .problems
            .is_empty()
    );
    assert!(
        !discover(
            &files(&format!("{REGISTRATION}\n{REGISTRATION}")),
            &[kind()]
        )
        .problems
        .is_empty()
    );
}

#[test]
fn saved_state_and_test_only_registrations_do_not_claim_a_spec_kind() {
    let text = format!(
        "{REGISTRATION}\nlink_files::specification!(Saved, include_str!(\"other.toml\"));\n#[cfg(test)] mod tests {{ link_spec::specification!(Fixture, dynamic_schema()); }}"
    );
    let found = discover(&files(&text), &[kind()]);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
}

#[test]
fn imported_registration_aliases_still_need_their_typed_kind() {
    let text = REGISTRATION.replace("link_spec::specification!", "register!");
    let text = format!("use link_spec::specification as register;\n{text}");
    assert!(discover(&files(&text), &[kind()]).problems.is_empty());
    assert!(!discover(&files(&text), &[]).problems.is_empty());
    assert!(
        !discover(
            &files("unknown::specification!(Example,include_str!(\"schema.toml\"));"),
            &[]
        )
        .problems
        .is_empty()
    );
}

#[test]
fn a_test_only_out_of_line_module_does_not_become_a_production_root() {
    let mut files = files(&format!("{REGISTRATION}\n#[cfg(test)] mod schema_cases;"));
    files.insert(
        "link-public/example/src/schema_cases.rs".into(),
        "link_spec::specification!(Fixture, dynamic_schema());".into(),
    );
    assert!(discover(&files, &[kind()]).problems.is_empty());
    let source = files
        .get_mut(Path::new("link-public/example/src/lib.rs"))
        .unwrap();
    *source = source.replace("#[cfg(test)]", "#[cfg(unix)]");
    assert!(!discover(&files, &[kind()]).problems.is_empty());
}

#[test]
fn presence_defaults_and_closed_choices_are_facts_not_guessed_runtime_cases() {
    let mut declared = kind();
    declared.shape["required"] = json!(["name"]);
    declared.shape["properties"]["name"]["enum"] = json!(["a", "b"]);
    let found = discover(&files(REGISTRATION), &[declared.clone()]);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let name = found
        .fields
        .iter()
        .find(|field| field.path == "name")
        .unwrap();
    assert_eq!(name.facts.presence, Presence::Required);
    assert_eq!(name.facts.required_when_present, Some(true));
    assert_eq!(
        name.facts.choices,
        json!(["a", "b"]).as_array().unwrap().clone()
    );
    let nested = found
        .fields
        .iter()
        .find(|field| field.path == "nested.value")
        .unwrap();
    assert_eq!(nested.facts.presence, Presence::Conditional);
    assert_eq!(nested.facts.required_when_present, Some(false));
    assert_eq!(nested.facts.default, Some(json!(false)));
    let name_content = name.content.clone();
    let nested_content = nested.content.clone();
    declared.shape["$defs"]["Nested"]["properties"]["value"]["default"] = json!(true);
    let changed = discover(&files(REGISTRATION), &[declared]);
    assert_eq!(
        changed
            .fields
            .iter()
            .find(|field| field.path == "name")
            .unwrap()
            .content,
        name_content
    );
    assert_ne!(
        changed
            .fields
            .iter()
            .find(|field| field.path == "nested.value")
            .unwrap()
            .content,
        nested_content
    );
}

#[test]
fn tagged_arms_with_the_same_field_path_have_stable_distinct_identities() {
    let mut declared = kind();
    for arm in declared.shape["properties"]["mode"]["oneOf"]
        .as_array_mut()
        .unwrap()
    {
        arm["properties"]["shared"] = json!({"type":"boolean"});
    }
    let found = discover(&files(REGISTRATION), &[declared.clone()]);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let branches: BTreeSet<_> = found
        .fields
        .iter()
        .filter(|field| field.path == "mode.shared")
        .map(|field| field.branch.clone())
        .collect();
    assert_eq!(branches.len(), 2);
    assert!(branches.iter().any(|branch| branch.contains("one")));
    assert!(branches.iter().any(|branch| branch.contains("two")));
    declared.shape["properties"]["mode"]["oneOf"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let moved = discover(&files(REGISTRATION), &[declared]);
    assert_eq!(
        branches,
        moved
            .fields
            .iter()
            .filter(|field| field.path == "mode.shared")
            .map(|field| field.branch.clone())
            .collect()
    );
}

#[test]
fn composed_fields_keep_every_constraint_and_required_obligation() {
    let mut declared = kind();
    declared.shape = json!({"allOf":[
        {"type":"object","properties":{"name":{"type":"string","enum":["a","b"]}}},
        {"type":"object","required":["name"],"properties":{"name":{"enum":["b","c"],"minLength":1}}}
    ]});
    let before = discover(&files(REGISTRATION), &[declared.clone()]);
    assert!(before.problems.is_empty(), "{:?}", before.problems);
    let name = before
        .fields
        .iter()
        .find(|field| field.path == "name")
        .unwrap();
    assert_eq!(name.facts.required_when_present, Some(true));
    assert_eq!(name.facts.choices, vec![json!("b")]);
    assert!(name.facts.constraints.contains(&"minLength".into()));
    let content = name.content.clone();
    declared.shape["allOf"][1]["properties"]["name"]["minLength"] = json!(2);
    let changed = discover(&files(REGISTRATION), &[declared.clone()]);
    assert_ne!(
        content,
        changed
            .fields
            .iter()
            .find(|field| field.path == "name")
            .unwrap()
            .content
    );
    declared.shape["allOf"][1]
        .as_object_mut()
        .unwrap()
        .remove("properties");
    let required_only = discover(&files(REGISTRATION), &[declared]);
    assert_eq!(
        required_only
            .fields
            .iter()
            .find(|field| field.path == "name")
            .unwrap()
            .facts
            .required_when_present,
        Some(true)
    );
}

#[test]
fn an_open_union_does_not_claim_exhaustive_choices_or_unconditional_constraints() {
    let mut declared = kind();
    declared.shape["properties"]["name"] =
        json!({"anyOf":[{"type":"string","enum":["known"]},{"type":"string"}]});
    let found = discover(&files(REGISTRATION), &[declared]);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let name = found
        .fields
        .iter()
        .find(|field| field.path == "name")
        .unwrap();
    assert!(name.facts.choices.is_empty());
    assert!(name.facts.constraints.is_empty());
    assert_eq!(name.facts.types, vec!["string"]);
    let mut declared = kind();
    declared.shape["properties"]["name"] = json!({"type":["boolean","null"]});
    let found = discover(&files(REGISTRATION), &[declared]);
    assert_eq!(
        found
            .fields
            .iter()
            .find(|field| field.path == "name")
            .unwrap()
            .facts
            .choices,
        vec![json!(false), json!(true)]
    );
}

#[test]
fn inline_untagged_object_arms_use_stable_schema_conditions() {
    let mut declared = kind();
    declared.shape["properties"]["mode"] = json!({"anyOf":[
        {"type":"object","properties":{"shared":{"type":"string"},"left":{"type":"boolean"}}},
        {"type":"object","properties":{"shared":{"type":"boolean"},"right":{"type":"boolean"}}}
    ]});
    let found = discover(&files(REGISTRATION), &[declared.clone()]);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let branches: BTreeSet<_> = found
        .fields
        .iter()
        .filter(|field| field.path == "mode.shared")
        .map(|field| field.branch.clone())
        .collect();
    assert_eq!(branches.len(), 2);
    declared.shape["properties"]["mode"]["anyOf"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let moved = discover(&files(REGISTRATION), &[declared]);
    assert_eq!(
        branches,
        moved
            .fields
            .iter()
            .filter(|field| field.path == "mode.shared")
            .map(|field| field.branch.clone())
            .collect()
    );
}

#[test]
fn constrained_and_overlapping_finite_declarations_need_reader_acceptance_review() {
    for schema in [
        json!({"type":"integer","enum":[1,2],"minimum":2}),
        json!({"type":"string","enum":["a","bb"],"minLength":2}),
        json!({"oneOf":[{"enum":["a","b"]},{"enum":["b","c"]}]}),
    ] {
        let mut declared = kind();
        declared.shape["properties"]["name"] = schema;
        let found = discover(&files(REGISTRATION), &[declared]);
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        let name = found
            .fields
            .iter()
            .find(|field| field.path == "name")
            .unwrap();
        assert!(!name.facts.choices.is_empty());
        assert!(name.facts.choices_need_review, "{:?}", name.facts);
    }
}

#[test]
fn root_alternative_required_fields_are_conditional_on_their_branch() {
    let mut declared = kind();
    declared.shape = json!({"oneOf":[
        {"type":"object","required":["kind","value"],"properties":{"kind":{"const":"one"},"value":{"type":"string"}}},
        {"type":"object","required":["kind","other"],"properties":{"kind":{"const":"two"},"other":{"type":"boolean"}}}
    ]});
    let found = discover(&files(REGISTRATION), &[declared]);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let value = found
        .fields
        .iter()
        .find(|field| field.path == "value")
        .unwrap();
    assert_eq!(value.facts.presence, Presence::Conditional);
    assert_eq!(value.facts.required_when_present, Some(true));
    assert!(value.branch.contains("one"));
}

#[test]
fn recursive_child_facts_stay_unknown_and_keep_their_parent_contract() {
    let mut declared = kind();
    declared.shape = json!({
        "type": "object",
        "default": {"enabled": true},
        "enum": [{"enabled": true}],
        "minLength": 7,
        "properties": {
            "enabled": {"type": "boolean", "default": false},
            "child": {"anyOf": [{"$ref": "#"}, {"type": "null"}]}
        }
    });
    let found = discover(&files(REGISTRATION), &[declared.clone()]);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let paths: BTreeSet<_> = found
        .fields
        .iter()
        .map(|field| field.path.as_str())
        .collect();
    assert_eq!(
        paths,
        BTreeSet::from(["child", "child.child", "child.enabled", "enabled"])
    );
    let direct = found
        .fields
        .iter()
        .find(|field| field.path == "enabled")
        .unwrap();
    assert_eq!(direct.facts.types, ["boolean"]);
    assert_eq!(direct.facts.default, Some(json!(false)));
    for path in ["child.enabled", "child.child"] {
        let field = found
            .fields
            .iter()
            .find(|field| field.path == path)
            .unwrap();
        assert_eq!(field.facts.presence, Presence::Unknown);
        assert_eq!(field.facts.required_when_present, None);
        assert!(field.facts.types.is_empty(), "{path}: {:?}", field.facts);
        assert!(field.facts.choices.is_empty(), "{path}: {:?}", field.facts);
        assert!(
            field.facts.constraints.is_empty(),
            "{path}: {:?}",
            field.facts
        );
        assert_eq!(field.facts.default, None);
        assert!(field.facts.flag.is_some());
        assert!(!field.facts.direct_flag);
        assert!(
            field.content["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node.get("recursive_parent").is_some())
        );
    }
    let child = |fields: Vec<SpecField>| {
        fields
            .into_iter()
            .find(|field| field.path == "child.enabled")
            .unwrap()
    };
    declared.shape["properties"]["enabled"] = json!({"type": "string", "default": "changed"});
    let changed = discover(&files(REGISTRATION), &[declared]);
    assert!(changed.problems.is_empty(), "{:?}", changed.problems);
    let before = child(found.fields);
    let after = child(changed.fields);
    assert!(after.facts.types.is_empty());
    assert_ne!(
        before.content, after.content,
        "changing the recursive parent still reopens the child's contract"
    );
}
