use serde_json::json;

use super::*;

fn format(before: &str) -> schema::Format {
    schema::Format::parse(&format!(
        "schema = 1\nkind = \"example\"\ncurrent = 1\ncommand = \"lnk example spec show\"\n{before}"
    ))
    .unwrap()
}

fn release(text: &str) -> link_version::Version {
    text.parse().unwrap()
}

#[test]
fn a_before_table_past_its_until_fails_its_kinds_check() {
    let before = format(
        "[before]\nsince = \"0.14.0\"\nuntil = \"0.16.0\"\n[before.keys]\nowner = \"leave it out\"\n",
    );
    assert!(check_format_at(&before, &release("0.15.0")).is_empty());
    let reached = check_format_at(&before, &release("0.16.0")).join("\n");
    assert!(
        reached.contains("[before] is deleted at 0.16.0"),
        "{reached}"
    );
    let early = check_format_at(&before, &release("0.13.0")).join("\n");
    assert!(early.contains("[before] comes at 0.14.0"), "{early}");
}

#[test]
fn a_before_key_the_current_shape_declares_is_a_problem() {
    let before = format(
        "[before]\nsince = \"0.14.0\"\nuntil = \"0.16.0\"\n[before.keys]\nowner = \"x\"\n\"jobs.*.id\" = \"y\"\n\"jobs.*.name\" = \"z\"\n",
    );
    let shape = json!({
        "type": "object",
        "properties": {
            "jobs": {"type": "array", "items": {"$ref": "#/$defs/Job"}}
        },
        "$defs": {"Job": {"type": "object", "properties": {"name": {"type": "string"}}}}
    });
    assert_eq!(
        check_before(&before, &shape),
        ["[before.keys] jobs.*.name is a key the current shape declares"]
    );
}

#[test]
fn a_step_past_its_until_names_the_code_to_delete_with_it() {
    // Built, as a step names a kind's code only in its `schema.toml`.
    let named: &'static str = Box::leak(format!("example_{}_to_{}", 1, 2).into_boxed_str());
    let code: &'static [schema::Migration] = Box::leak(Box::new([schema::Migration {
        name: named,
        run: Ok,
    }]));
    let format = schema::Format::parse_with(
        &format!(
            "schema = 1\nkind = \"example\"\ncurrent = 2\ncommand = \"lnk example spec show\"\n[[steps]]\n\
             from = 1\nsince = \"0.14.0\"\nuntil = \"0.16.0\"\nmigration = \"{named}\"\n"
        ),
        code,
    )
    .unwrap();
    assert!(check_format_at(&format, &release("0.15.0")).is_empty());
    let reached = check_format_at(&format, &release("0.16.0")).join("\n");
    assert!(
        reached.contains(&format!("delete the step and its code, {named},")),
        "{reached}"
    );
}
