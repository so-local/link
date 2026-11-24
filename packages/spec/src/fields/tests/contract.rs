//! Regression checks for spec field serialization and arbitrary keys.
use super::*;

#[test]
fn opaque_text_that_looks_like_toml_keeps_its_type_and_quotes_when_forwarded() {
    let spec = Odd {
        anys: [
            "true",
            "1",
            "1.5",
            "\"quoted\"",
            "'literal'",
            "[false]",
            "{a=1}",
            "1979-05-27",
        ]
        .into_iter()
        .map(|value| serde_json::json!(value))
        .collect(),
        ..Odd::default()
    };
    let words = crate::flags_of(&spec).unwrap();
    let borrowed: Vec<&str> = words.iter().map(String::as_str).collect();
    let again: Odd = toml::Value::Table(odd(&borrowed).unwrap())
        .try_into()
        .unwrap();
    assert_eq!(again, spec, "{words:?}");
}

#[test]
fn flag_shaped_text_values_remain_data_when_forwarded() {
    let spec = Odd {
        meta: BTreeMap::from([("argument".into(), serde_json::json!("--spec.name"))]),
        anys: vec![serde_json::json!("--stdio"), serde_json::json!("-x")],
        ..Odd::default()
    };
    let words = crate::flags_of(&spec).unwrap();
    let borrowed: Vec<&str> = words.iter().map(String::as_str).collect();
    let again: Odd = toml::Value::Table(odd(&borrowed).unwrap())
        .try_into()
        .unwrap();
    assert_eq!(again, spec, "{words:?}");
}

#[test]
fn escaped_quotes_in_free_keys_keep_equals_inside_the_key() {
    for (flag, key) in [
        (r#"--tool."a\"=b".limit=4"#, "a\"=b"),
        (r#"--tool."a\\\"=b".limit=4"#, "a\\\"=b"),
        (r#"--tool.'a"=b'.limit=4"#, "a\"=b"),
    ] {
        let given = flags(&[flag]);
        assert_eq!(given.table()["tool"][key]["limit"].as_integer(), Some(4));
        let spec = over(&Agent::default(), &given).unwrap();
        let again = crate::flags_of(&spec).unwrap();
        let words: Vec<&str> = again.iter().map(String::as_str).collect();
        assert_eq!(over(&Agent::default(), &flags(&words)).unwrap(), spec);
    }
}

#[test]
fn a_true_value_in_an_opaque_map_keeps_its_value_when_forwarded() {
    let table = odd(&["--meta.headless=true"]).unwrap();
    let spec: Odd = toml::Value::Table(table).try_into().unwrap();
    let words = crate::flags_of(&spec).unwrap();
    let borrowed: Vec<&str> = words.iter().map(String::as_str).collect();
    let again: Odd = toml::Value::Table(odd(&borrowed).unwrap())
        .try_into()
        .unwrap();
    assert_eq!(again, spec);
}

#[test]
fn a_nested_list_takes_whole_lists_elements_and_an_empty_list_without_ambiguity() {
    for values in [vec![], vec![vec![]], vec![vec!["a".to_string()], vec![]]] {
        let spec = Odd {
            grid: values,
            ..Odd::default()
        };
        let words = crate::flags_of(&spec).unwrap();
        let borrowed: Vec<&str> = words.iter().map(String::as_str).collect();
        let again: Odd = toml::Value::Table(odd(&borrowed).unwrap())
            .try_into()
            .unwrap();
        assert_eq!(again, spec, "{words:?}");
    }
    assert_eq!(
        odd(&["--grid", r#"[["a"], ["b"]]"#]).unwrap()["grid"],
        odd(&["--grid", r#"["a"]"#, "--grid", r#"["b"]"#]).unwrap()["grid"]
    );
}
