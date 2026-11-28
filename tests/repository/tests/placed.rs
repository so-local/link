//! Every format Link saves says where its files are, or that it holds
//! history (`link_repository_check::placed`, by `placed.toml`).

use std::path::Path;

use link_repository_check::placed::{
    Declared, at_a_location, declared, formats, made, placed, problems,
};

#[test]
fn every_saved_format_is_placed_but_the_listed() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let problems = problems(&root, &here.join("placed.toml"));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn a_format_is_declared_by_its_files_macro_or_a_shipped_format() {
    let text = r#"
link_files::specification!(Kept, include_str!("kept.schema.toml"));
link_files::specification!(
    Other,
    include_str!("other/schema.toml")
);
static F: LazyLock<Format> = LazyLock::new(|| {
    link_files::schema::Format::shipped(include_str!("shipped.schema.toml"))
});
link_spec::specification!(Spec, include_str!("spec.schema.toml"));
crate::specification!(Own, include_str!("own.schema.toml"));
#[cfg(test)]
mod tests {
    link_files::specification!(Fixture, include_str!("fixture.schema.toml"));
}
"#;
    assert_eq!(
        declared("modules/x/src/lib.rs", text),
        [
            "kept.schema.toml",
            "other/schema.toml",
            "shipped.schema.toml"
        ]
    );
    assert_eq!(
        declared("packages/files/src/lib.rs", text),
        [
            "kept.schema.toml",
            "other/schema.toml",
            "own.schema.toml",
            "shipped.schema.toml"
        ]
    );
}

#[test]
fn a_format_is_placed_by_its_files_as_history_or_inside() {
    let files = "schema = 1\nkind = \"k\"\ncurrent = 1\n[[files]]\nlocation = \"lnk.env\"\nencoding = \"toml\"\n";
    assert_eq!(placed(files), Ok(true));
    let history = "schema = 1\nkind = \"k\"\ncurrent = 1\nholds = \"history\"\n";
    assert_eq!(placed(history), Ok(true));
    let inside = "schema = 1\nkind = \"k\"\ncurrent = 1\nholds = \"inside\"\n";
    assert_eq!(placed(inside), Ok(true));
    assert_eq!(placed("schema = 1\nkind = \"k\"\ncurrent = 1\n"), Ok(false));
    assert_eq!(
        placed("schema = 1\nkind = \"k\"\ncurrent = 1\nholds = \"state\"\n"),
        Ok(false)
    );
}

#[test]
fn a_format_at_a_location_is_made_a_saved_in_its_crate() {
    let files = "schema = 1\nkind = \"k\"\ncurrent = 1\n[[files]]\nlocation = \"lnk.env\"\nencoding = \"toml\"\n";
    assert_eq!(at_a_location(files), Ok(true));
    let inside = "schema = 1\nkind = \"k\"\ncurrent = 1\nholds = \"inside\"\n";
    assert_eq!(at_a_location(inside), Ok(false));
    let history = files.replace("[[files]]", "holds = \"history\"\n[[files]]");
    assert_eq!(at_a_location(&history), Ok(false));
    let kept = formats(
        "modules/x/src/lib.rs",
        "link_files::specification!(store::Kept, include_str!(\"kept.schema.toml\"));\n",
    );
    let kept = &kept[0];
    assert!(made(kept, "Saved::of::<Kept>()"));
    assert!(made(
        kept,
        "link_files::upgrade::Saved::of::<crate::store::Kept>()"
    ));
    assert!(!made(kept, "Saved::of::<Other>()"));
    assert!(!made(kept, "fn saved() {}"));
    let shipped = Declared {
        ty: None,
        schema: "held.schema.toml".into(),
    };
    assert!(made(&shipped, "Saved::shipped::<Held>(&HELD)"));
    assert!(!made(&shipped, "Saved::of::<Held>()"));
}
