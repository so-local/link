//! IDs in a command's output: each `<kind>:<id>`, under a field naming
//! its axis (`link_id::axes`), never bare. One check every test of a
//! `--json` calls, so no command writes an ID another way.

use serde_json::Value;

/// [`ids.toml`](ids.toml), read once.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    labels: Vec<String>,
}

fn labels() -> &'static [String] {
    static SETTINGS: std::sync::LazyLock<Settings> = std::sync::LazyLock::new(|| {
        toml::from_str(include_str!("ids.toml")).expect("the test kit's ids.toml reads")
    });
    &SETTINGS.labels
}

/// Fails, naming each, on every ID in `value` written outside its form:
/// a bare candidate (`link_id::is_candidate`), or `<kind>:<candidate>`
/// (an execution's `@<run>` after it) under a field that names no axis.
/// A list's items are under its field; an object's keys are checked as
/// strings under the object's field.
pub fn assert_named(value: &Value) {
    let mut wrong = Vec::new();
    walk(value, None, "$", &mut wrong);
    assert!(
        wrong.is_empty(),
        "IDs outside <kind>:<id> under a field naming its axis ({}):\n{}",
        link_id::axes().join(", "),
        wrong.join("\n")
    );
}

/// [`assert_named`] over a command's standard output: one JSON value, or
/// one per line, as a command that replies line by line writes them.
pub fn assert_named_output(stdout: &str) {
    if let Ok(value) = serde_json::from_str::<Value>(stdout) {
        return assert_named(&value);
    }
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line)
            .unwrap_or_else(|err| panic!("not a JSON line ({err}): {line}"));
        assert_named(&value);
    }
}

fn walk(value: &Value, field: Option<&str>, at: &str, wrong: &mut Vec<String>) {
    match value {
        Value::String(text) => check(text, field, at, wrong),
        Value::Array(items) => {
            for (n, item) in items.iter().enumerate() {
                walk(item, field, &format!("{at}[{n}]"), wrong);
            }
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                check(key, field, &format!("{at} (a key)"), wrong);
                walk(item, Some(key), &format!("{at}.{key}"), wrong);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn check(text: &str, field: Option<&str>, at: &str, wrong: &mut Vec<String>) {
    // Labels (a name, a sandbox's folder names) stay labels, even when a
    // machine's hostname or a folder has the shape of a minted ID. Only
    // ID-bearing fields promise an identity here.
    let label = field.is_some_and(|f| labels().iter().any(|l| l == f));
    if !label && link_id::is_candidate(text) {
        wrong.push(format!("{at}: {text:?} is a bare ID"));
        return;
    }
    // An execution, `<workload>@<run>`, is its workload's ID first.
    let id = text
        .split_once(link_id::RUN_SEPARATOR)
        .map_or(text, |(id, _)| id);
    let Some((kind, id)) = id.split_once(':') else {
        return;
    };
    if link_id::is_kind_name(kind)
        && link_id::is_candidate(id)
        && !field.is_some_and(link_id::is_axis)
    {
        let field = field.unwrap_or("no field");
        wrong.push(format!(
            "{at}: {text:?} is under {field:?}, which names no axis"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_ids_pass() {
        assert_named(&serde_json::json!({
            "workload": "agent:a1b2c3d4e5f6",
            "execution": "agent:a1b2c3d4e5f6@7",
            "teams": [{"resource": "group:a1b2c3d4e5f6"}],
            "resource": ["box:a1b2c3d4e5f6"],
            "name": "main",
            "id": "i-0123456789abcdef0",
            "port": 8080,
        }));
        assert_named_output("{\"resource\":\"group:a1b2c3d4e5f6\"}\n{\"name\":\"research\"}\n");
    }

    #[test]
    fn a_name_with_the_shape_of_an_id_is_still_a_label() {
        assert_named(&serde_json::json!({"machine": {
            "name": "e43a1ff327b7", "resource": "installation:a1b2c3d4e5f6"
        }}));
        let said = refused(serde_json::json!({"machine": {
            "name": "e43a1ff327b7", "resource": "a1b2c3d4e5f6"
        }}));
        assert!(said.contains("$.machine.resource"), "{said}");
    }

    /// A sandbox's folder names are labels: `instructions` has the shape
    /// of an ID, and a `[files]` list holds it.
    #[test]
    fn a_folder_with_the_shape_of_an_id_is_still_a_label() {
        assert_named(&serde_json::json!({"spec": {"files": {
            "write": ["work", "instructions", "skills"], "read": ["chats"]
        }}}));
    }

    fn refused(value: Value) -> String {
        let panic = std::panic::catch_unwind(|| assert_named(&value)).expect_err("refused");
        panic.downcast_ref::<String>().cloned().expect("a message")
    }

    #[test]
    fn a_bare_id_is_refused() {
        let said = refused(serde_json::json!({"agents": [{"id": "a1b2c3d4e5f6"}]}));
        assert!(said.contains("$.agents[0].id"), "{said}");
    }

    #[test]
    fn an_id_under_a_field_naming_no_axis_is_refused() {
        let said = refused(serde_json::json!({"teams": ["group:a1b2c3d4e5f6"]}));
        assert!(
            said.contains("$.teams[0]") && said.contains("\"teams\""),
            "{said}"
        );
        refused(serde_json::json!({"id": "agent:a1b2c3d4e5f6@7"}));
        refused(serde_json::json!("box:a1b2c3d4e5f6"));
    }

    #[test]
    fn an_id_as_a_key_is_refused() {
        let said = refused(serde_json::json!({"measured": {"a1b2c3d4e5f6": 1}}));
        assert!(said.contains("$.measured (a key)"), "{said}");
    }
}
