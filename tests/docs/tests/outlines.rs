//! The persisted site outline and README tree share discovery and order.
//! New pages cannot depend on runtime-only navigation augmentation.

link_test_kit::program!(unit);

use std::path::{Path, PathBuf};

use link_docs_check::Tree;
use link_docs_check::navigation::{Navigation, check};
use link_docs_check::outlines;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("outline-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let fixture = Self { root };
        fixture.write("large-files.toml", "[lines]\n");
        fixture.write("README.md", "# Repository\n\nRoot.\n");
        fixture.write("scripts/documentation-index.json", r#"{"sites":[{"name":"Docs","url":"https://docs.example","directory":"web","roots":true}],"roots":[{"directory":"a"},{"directory":"b"},{"directory":"."}]}"#);
        fixture.write("web/README.md", "# Website\n\nThe website.\n");
        fixture.write("a/README.md", "# A package\n\nPackage A.\n");
        fixture.write("b/README.md", "# B package\n\nPackage B.\n");
        fixture.write("a/docs/a/README.md", "# A\n\nFeature A.\n");
        fixture.write("b/docs/b/README.md", "# B\n\nFeature B.\n");
        fixture.write("web/docs/navigation.md", "# Docs\n\n## Pages\n\n- [B](../../b/docs/b/README.md)\n- [A](../../a/docs/a/README.md)\n");
        fixture
    }
    fn write(&self, path: &str, text: &str) {
        let path = self.root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.root.join(path)).unwrap()
    }
    fn tree(&self) -> Tree {
        Tree::read(&self.root, false, &self.root.join("large-files.toml"))
    }
    fn generate(&self) {
        for update in Navigation::new(&self.tree()).updates().unwrap() {
            std::fs::create_dir_all(update.path.parent().unwrap()).unwrap();
            std::fs::write(update.path, update.text).unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn section_outlines_preserve_order_and_receive_discovered_pages_without_becoming_pages() {
    let fixture = Fixture::new("section-outlines");
    fixture.write("web/docs/navigation.md", "# Docs\n\n## Pages\n\n- [B](navigation/b.md \"include\")\n- [A](../../a/docs/a/README.md)\n");
    fixture.write(
        "web/docs/navigation/b.md",
        "# B Navigation\n\n## Pages\n\n- [B](../../../b/docs/b/README.md)\n",
    );
    fixture.write(
        "b/docs/b/new.md",
        "# New Capability\n\nAnother capability.\n",
    );
    fixture.generate();
    assert!(
        fixture
            .read("web/docs/navigation.md")
            .contains("[B](navigation/b.md \"include\")")
    );
    assert!(
        !fixture
            .read("web/docs/navigation.md")
            .contains("New Capability")
    );
    assert!(
        fixture
            .read("web/docs/navigation/b.md")
            .contains("  - [New Capability](../../../b/docs/b/new.md)")
    );
    let tree = fixture.tree();
    assert!(!tree.pages.keys().any(|url| url.contains("/navigation/")));
    let nodes = outlines::expanded(&tree, &fixture.root.join("web/docs/navigation.md")).unwrap();
    assert_eq!(nodes[0].label, "B");
    assert_eq!(nodes[0].children[0].label, "New Capability");
    assert!(check(&tree).is_empty());
}

#[test]
fn section_outlines_refuse_missing_duplicate_nested_unreferenced_and_escaping_includes() {
    let valid = "# B Navigation\n\n## Pages\n\n- [B](../../../b/docs/b/README.md)\n";
    for (index, (entry, part, reason)) in [
        (
            "- [B](navigation/b.md \"include\")",
            None,
            "missing outline",
        ),
        ("- [B](../b.md \"include\")", None, "must name navigation"),
        (
            "- [B](navigation/b.md \"include\")\n- [B](navigation/b.md \"include\")",
            Some(valid),
            "duplicate outline include",
        ),
        (
            "- [B](navigation/b.md \"include\")",
            Some("# B Navigation\n\n## Pages\n\n- [B](navigation/b.md \"include\")\n"),
            "never nested",
        ),
        (
            "- [B](../../b/docs/b/README.md)",
            Some(valid),
            "unreferenced outline",
        ),
        (
            "- [B](navigation/b.md \"include\")",
            Some("# B Navigation\n\n## Pages\n\n- [B](b.md)\n- [A](a.md)\n"),
            "one top-level B",
        ),
        (
            "- [B](navigation/b.md \"include\")",
            Some("# B Navigation\n\nProse.\n\n## Pages\n\n- [B](b.md)\n"),
            "contains prose",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let fixture = Fixture::new(&format!("invalid-outline-section-{index}"));
        fixture.write(
            "web/docs/navigation.md",
            &format!("# Docs\n\n## Pages\n\n{entry}\n"),
        );
        if let Some(part) = part {
            fixture.write("web/docs/navigation/b.md", part);
        }
        let error = match Navigation::new(&fixture.tree()).updates() {
            Ok(_) => panic!("accepted invalid section outline"),
            Err(error) => error,
        };
        assert!(error.contains(reason), "{error}");
    }
}

#[test]
fn new_docs_are_persisted_beneath_the_nearest_listed_ancestor() {
    let fixture = Fixture::new("insertion");
    fixture.write("a/docs/a/reference.md", "# Reference\n\nThe interface.\n");
    assert!(!check(&fixture.tree()).is_empty());
    fixture.generate();
    let outline = fixture.read("web/docs/navigation.md");
    assert!(
        outline.contains(
            "- [A](../../a/docs/a/README.md)\n  - [Reference](../../a/docs/a/reference.md)"
        )
    );
    assert!(outline.find("- [B]").unwrap() < outline.find("- [A]").unwrap());
    assert!(
        Navigation::new(&fixture.tree())
            .updates()
            .unwrap()
            .is_empty()
    );
    assert!(Navigation::new(&fixture.tree()).unreachable().is_empty());
}

#[test]
fn readme_indexes_read_the_curated_order_and_folders_alphabetically() {
    let fixture = Fixture::new("order");
    fixture.write("a/docs/a/z.md", "# Z\n\nLast alphabetically.\n");
    fixture.write("a/docs/a/c.md", "# C\n\nFirst alphabetically.\n");
    fixture.write("web/docs/navigation.md", "# Docs\n\n## Pages\n\n- [B](../../b/docs/b/README.md)\n- [A](../../a/docs/a/README.md)\n  - [Z](../../a/docs/a/z.md)\n  - [C](../../a/docs/a/c.md)\n");
    let original = fixture.read("web/docs/navigation.md");
    fixture.generate();
    assert_eq!(fixture.read("web/docs/navigation.md"), original);
    let root = fixture.read("README.md");
    assert!(root.find("[A package]").unwrap() < root.find("[B package]").unwrap());
    // The website's pages in its order; folders it doesn't hold, by title.
    let a = fixture.read("a/docs/a/README.md");
    assert!(a.find("[Z]").unwrap() < a.find("[C]").unwrap());
    let texts: Vec<_> = fixture.tree().files.into_iter().collect();
    fixture.generate();
    assert_eq!(fixture.tree().files.into_iter().collect::<Vec<_>>(), texts);
}

#[test]
fn new_package_subtrees_are_added_without_reparenting_curated_entries() {
    let fixture = Fixture::new("new-package");
    fixture.write("scripts/documentation-index.json", r#"{"sites":[{"name":"Docs","url":"https://docs.example","directory":"web","roots":true}],"roots":[{"directory":"a"},{"directory":"b"},{"directory":"new"}]}"#);
    fixture.write("new/Cargo.toml", "[package]\nname='new'\n");
    fixture.write("new/README.md", "# New package\n\nNew package.\n");
    fixture.write("new/docs/new/README.md", "# New\n\nNew feature.\n");
    fixture.write("new/docs/new/details.md", "# Details\n\nNew details.\n");
    fixture.generate();
    let outline = fixture.read("web/docs/navigation.md");
    assert!(outline.ends_with(
        "- [New](../../new/docs/new/README.md)\n  - [Details](../../new/docs/new/details.md)\n"
    ));
    assert!(outline.find("- [B]").unwrap() < outline.find("- [A]").unwrap());
    assert!(
        fixture
            .read("README.md")
            .contains("[New package](new/README.md)")
    );
}

#[test]
fn duplicate_and_dangling_entries_are_rejected_before_any_write() {
    let fixture = Fixture::new("invalid");
    for (entries, reason) in [
        (
            "- [A](../../a/docs/a/README.md)\n- [Again](../../a/docs/a/README.md)\n",
            "duplicate page",
        ),
        ("- [Missing](../../a/docs/missing.md)\n", "dangling"),
        ("- [Missing](https://docs.example/missing)\n", "dangling"),
    ] {
        fixture.write(
            "web/docs/navigation.md",
            &format!("# Docs\n\n## Pages\n\n{entries}"),
        );
        let original = fixture.read("README.md");
        let error = Navigation::new(&fixture.tree()).updates().unwrap_err();
        assert!(error.contains(reason), "{error}");
        assert_eq!(fixture.read("README.md"), original);
    }
}

#[test]
fn runtime_markdown_and_metadata_are_excluded_but_navigation_guides_are_pages() {
    let fixture = Fixture::new("exclusions");
    fixture.write("a/Cargo.toml", "[package]\nname='a'\n");
    fixture.write("a/commands/prompt.md", "# Prompt\n\nRuntime data.\n");
    fixture.write(
        "a/docs/a/navigation.md",
        "# Navigation guide\n\nA real guide.\n",
    );
    fixture.write("web/docs/navigation.es.md", "# Docs\n\n## Pages\n\n");
    fixture.generate();
    let tree = fixture.tree();
    let pages: Vec<_> = tree.pages.values().flatten().collect();
    assert!(!pages.iter().any(|p| p.ends_with("commands/prompt.md")));
    assert!(!pages.iter().any(|p| p.ends_with("web/docs/navigation.md")));
    assert!(pages.iter().any(|p| p.ends_with("a/docs/a/navigation.md")));
    let outline = fixture.read("web/docs/navigation.md");
    assert!(outline.contains("[Navigation guide]"));
    assert!(!outline.contains("prompt.md"));
    assert!(!fixture.read("web/README.md").contains("navigation.md"));
}

#[test]
fn external_and_cross_site_entries_retain_their_curated_positions() {
    let fixture = Fixture::new("external");
    fixture.write("scripts/documentation-index.json", r#"{"sites":[{"name":"Docs","url":"https://docs.example","directory":"web","roots":true},{"name":"Home","url":"https://home.example","directory":"home"}],"roots":[{"directory":"a"},{"directory":"b"}]}"#);
    fixture.write("home/docs/README.md", "# Home\n\nHome page.\n");
    fixture.write(
        "home/docs/navigation.md",
        "# Home\n\n## Pages\n\n- [Home](README.md)\n",
    );
    fixture.write("web/docs/navigation.md", "# Docs\n\n## Pages\n\n- [External](https://example.com/reference)\n- [Home](https://home.example)\n- [A](../../a/docs/a/README.md)\n- [B](../../b/docs/b/README.md)\n");
    let original = fixture.read("web/docs/navigation.md");
    fixture.generate();
    assert_eq!(fixture.read("web/docs/navigation.md"), original);
    let tree = fixture.tree();
    let rules: Vec<_> = link_docs_check::rules()
        .into_iter()
        .filter(|r| r.id == "links")
        .collect();
    link_docs_check::run(&tree, &rules).assert();
}

#[test]
fn structural_metadata_rejects_prose_and_accepts_translation_url_mappings() {
    assert!(outlines::structure("# Site\n\nA description.\n\n## Pages\n\n", false).is_err());
    assert!(outlines::structure("# Site\n\n## Pages\n\n## Words\n", false).is_err());
    assert!(
        outlines::structure(
            "# Site\n\n## Pages\n\n- [Page](page.md)\n\n## URLs\n\n- english-slug: translated-slug\n",
            true
        )
        .is_ok()
    );
}

#[test]
fn each_app_keeps_its_own_outline_and_its_readme_indexes_follow_it() {
    let fixture = Fixture::new("two-sites");
    fixture.write("scripts/documentation-index.json", r#"{"sites":[{"name":"Home","url":"https://home.example","directory":"home"},{"name":"Docs","url":"https://docs.example","directory":"web","roots":true}],"roots":[{"directory":"a"},{"directory":"b"}]}"#);
    fixture.write("home/README.md", "# Marketing\n\nHome app.\n");
    fixture.write("home/docs/README.md", "# Home\n\nHome page.\n");
    fixture.write("home/docs/z.md", "# Z\n\nFirst curated.\n");
    fixture.write("home/docs/c.md", "# C\n\nLast curated.\n");
    fixture.write(
        "home/docs/navigation.md",
        "# Home\n\n## Pages\n\n- [Home](README.md)\n  - [Z](z.md)\n  - [C](c.md)\n",
    );
    fixture.write("web/docs/navigation.md", "# Docs\n\n## Pages\n\n- [B](../../b/docs/b/README.md)\n- [A](../../a/docs/a/README.md)\n- [Marketing home](https://home.example)\n");
    fixture.generate();
    let root = fixture.read("README.md");
    assert!(root.find("[A package]").unwrap() < root.find("[B package]").unwrap());
    assert!(root.find("[B package]").unwrap() < root.find("[Marketing]").unwrap());
    let home = fixture.read("home/docs/README.md");
    assert!(home.find("[Z]").unwrap() < home.find("[C]").unwrap());
    let outline = fixture.read("home/docs/navigation.md");
    assert!(outline.find("[Z]").unwrap() < outline.find("[C]").unwrap());
}

#[test]
fn title_changes_refresh_labels_but_keep_get_started_and_cross_site_labels() {
    let fixture = Fixture::new("labels");
    fixture.write("a/docs/a/README.md", "# New A\n\nFeature A.\n");
    fixture.write("b/docs/b/README.md", "# New B\n\nFeature B.\n");
    fixture.write(
        "a/docs/presale.md",
        "# Local Link Presale\n\nThe presale.\n",
    );
    fixture.write("web/docs/navigation.md", "# Docs\n\n## Pages\n\n- [Get Started](../../b/docs/b/README.md)\n- [A](../../a/docs/a/README.md)\n- [Presale](../../a/docs/presale.md \"tab\")\n- [External name](https://example.com)\n");
    fixture.generate();
    let outline = fixture.read("web/docs/navigation.md");
    assert!(outline.contains("[Get Started]"));
    assert!(outline.contains("[New A]"));
    assert!(outline.contains("[Presale](../../a/docs/presale.md \"tab\")"));
    assert!(outline.contains("[External name]"));
    assert!(
        Navigation::new(&fixture.tree())
            .updates()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn checkout_root_loads_the_child_workspace_outline_and_missing_outline_is_created() {
    let fixture = Fixture::new("checkout");
    std::fs::create_dir_all(fixture.root.join("public")).unwrap();
    for path in ["scripts", "a", "b", "web"] {
        std::fs::rename(
            fixture.root.join(path),
            fixture.root.join("public").join(path),
        )
        .unwrap();
    }
    fixture.write("public/README.md", "# Public\n\nPublic workspace.\n");
    std::fs::remove_file(fixture.root.join("public/web/docs/navigation.md")).unwrap();
    fixture.generate();
    assert!(
        fixture
            .read("public/web/docs/navigation.md")
            .contains("[A]")
    );
    assert!(
        fixture
            .read("public/web/docs/navigation.md")
            .contains("[B]")
    );
    assert!(
        Navigation::new(&fixture.tree())
            .updates()
            .unwrap()
            .is_empty()
    );
    assert!(Navigation::new(&fixture.tree()).unreachable().is_empty());
}

#[test]
fn unregistered_english_owner_is_an_error_instead_of_a_silent_omission() {
    let fixture = Fixture::new("unregistered");
    fixture.write(
        "unregistered/Cargo.toml",
        "[package]\nname='unregistered'\n",
    );
    fixture.write("unregistered/docs/new.md", "# New\n\nNew public page.\n");
    let error = Navigation::new(&fixture.tree()).updates().unwrap_err();
    assert!(error.contains("not registered"), "{error}");
}

#[test]
fn unregistered_group_folder_owner_is_an_error_too() {
    for (name, marker, text) in [
        ("unregistered-layer", "layer.toml", "depends_on = []\n"),
        (
            "unregistered-readme",
            "README.md",
            "# Group\n\nA group of crates.\n",
        ),
    ] {
        let fixture = Fixture::new(name);
        fixture.write(&format!("group/{marker}"), text);
        fixture.write("group/docs/group.md", "# Group\n\nThe group's page.\n");
        let error = Navigation::new(&fixture.tree()).updates().unwrap_err();
        assert!(error.contains("not registered"), "{marker}: {error}");
    }
}
