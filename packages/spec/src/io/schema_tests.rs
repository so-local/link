use super::*;
use link_test_kit::TempDir;

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Example {
    name: String,
}
crate::specification!(
    Example,
    include_str!("../../tests/fixtures/spec-schema.toml")
);

fn converted(path: &Path) -> anyhow::Result<Converted> {
    let exported = crate::Exported::of::<Example>();
    convert(&exported.format(), &exported.shape, path)
}

#[test]
fn ordinary_reads_preserve_source_and_conversion_keeps_comments() {
    let dir = TempDir::new("spec-rewrite");
    let path = dir.join("spec.toml");
    let source = "# My settings\nschema = 1\n# My name\ntitle = 'web' # keep\n";
    std::fs::write(&path, source).unwrap();
    let read = read_spec::<Example>(&path).unwrap();
    assert_eq!(read.spec.name, "web");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    converted(&path).unwrap();
    let current = std::fs::read_to_string(&path).unwrap();
    for words in [
        "# My settings",
        "# My name",
        "# keep",
        "kind = \"example\"",
        "schema = 2",
        "name = 'web'",
    ] {
        assert!(current.contains(words), "{current}");
    }
    assert_eq!(read_spec::<Example>(&path).unwrap().spec.name, "web");
}

#[test]
fn failed_conversions_leave_source_intact() {
    let dir = TempDir::new("spec-refused");
    let path = dir.join("spec.toml");
    for source in [
        "schema = 3\nname = 'future'\n",
        "schema = 1\ntitle = 'web'\nname = 'other'\n",
        "schema = 1\ntitle = 7\n",
        "kind = 'other'\nname = 'web'\n",
    ] {
        std::fs::write(&path, source).unwrap();
        assert!(converted(&path).is_err(), "{source}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    }
}
