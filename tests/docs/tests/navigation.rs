//! Navigation checks use the writer's discovery and rendering, exercised
//! against folders whose omissions previously passed the hub checks.

link_test_kit::program!(unit);

use std::path::PathBuf;

use link_docs_check::Tree;
use link_docs_check::navigation::{END, Navigation, START, check, reachability};

struct Fixture {
    root: PathBuf,
    _dir: link_test_kit::TempDir,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = link_test_kit::TempDir::new(&format!("navigation-{name}"));
        let fixture = Self {
            root: dir.path().to_path_buf(),
            _dir: dir,
        };
        fixture.write("large-files.toml", "[lines]\n");
        fixture
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn tree(&self) -> Tree {
        Tree::read(&self.root, false, &self.root.join("large-files.toml"))
    }

    fn generate(&self) {
        for update in Navigation::new(&self.tree()).updates().unwrap() {
            std::fs::write(update.path, update.text).unwrap();
        }
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.root.join(path)).unwrap()
    }
}

#[test]
fn a_shared_homepage_is_the_site_root_and_keeps_one_reading_path() {
    let fixture = Fixture::new("shared-homepage");
    fixture.write("README.md", "# Product\n\nThe product introduction.\n");
    fixture.write("web/README.md", "# Website\n\nThe website.\n");
    fixture.write(
        "scripts/documentation-index.json",
        r#"{"sites":[{"name":"Docs","url":"https://docs.example","directory":"web","homepage":"README.md","roots":true}],"roots":[]}"#,
    );
    fixture.write(
        "web/docs/navigation.md",
        "# Docs\n\n## Pages\n\n- [Get Started](../../README.md)\n",
    );
    fixture.generate();
    let tree = fixture.tree();
    assert_eq!(
        tree.pages["https://docs.example"],
        [fixture.root.join("README.md")]
    );
    assert!(check(&tree).is_empty());
    assert!(Navigation::new(&tree).unreachable().is_empty());
    assert!(!fixture.root.join("web/docs/README.md").exists());
}

#[test]
fn authored_docs_heading_and_introduction_stay_above_the_generated_links() {
    let fixture = Fixture::new("authored-docs-introduction");
    fixture.write(
        "README.md",
        &format!("# Root\n\nThe product.\n\n## Docs\n\nRead online or with the CLI.\n\n{START}\n\n{END}\n"),
    );
    fixture.write("guide.md", "# Guide\n\nRead the guide.\n");
    fixture.generate();
    let root = fixture.read("README.md");
    assert_eq!(root.matches("## Docs").count(), 1);
    assert!(root.contains(&format!(
        "## Docs\n\nRead online or with the CLI.\n\n{START}\n\n- [Guide](guide.md): Read the guide"
    )));
    fixture.write("guide.md", "# New Guide\n\nRead the updated guide.\n");
    fixture.generate();
    let updated = fixture.read("README.md");
    assert_eq!(updated.matches("## Docs").count(), 1);
    assert!(updated.contains("Read online or with the CLI."));
    assert!(updated.contains("[New Guide](guide.md): Read the updated guide"));
    assert!(
        Navigation::new(&fixture.tree())
            .updates()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn folder_index_is_alphabetical_without_reordering_website_pages() {
    let fixture = Fixture::new("workspace-folder-order");
    fixture.write("Cargo.toml", "[workspace]\nmembers = []\n");
    fixture.write("README.md", "# Product\n\nThe product.\n");
    fixture.write("core/README.md", "# Core\n\nThe core.\n");
    fixture.write("modules/README.md", "# Modules\n\nThe modules.\n");
    fixture.write("core/docs/core/README.md", "# Core\n\nUse the core.\n");
    fixture.write(
        "modules/docs/modules/README.md",
        "# Modules\n\nUse modules.\n",
    );
    fixture.write("web/README.md", "# Website\n\nThe website.\n");
    fixture.write(
        "scripts/documentation-index.json",
        r#"{"sites":[{"name":"Docs","url":"https://docs.example","directory":"web","roots":true}],"roots":[{"directory":"core"},{"directory":"modules"}]}"#,
    );
    let outline = "# Docs\n\n## Pages\n\n- [Modules](../../modules/docs/modules/README.md)\n- [Core](../../core/docs/core/README.md)\n";
    fixture.write("web/docs/navigation.md", outline);
    fixture.generate();
    let root = fixture.read("README.md");
    assert!(root.find("[Core]").unwrap() < root.find("[Modules]").unwrap());
    assert_eq!(fixture.read("web/docs/navigation.md"), outline);
    assert!(
        Navigation::new(&fixture.tree())
            .updates()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn test_fixture_readmes_are_discoverable_without_indexing_fixture_data() {
    let fixture = Fixture::new("fixture-readmes");
    fixture.write("README.md", "# Root\n\nThe product.\n");
    fixture.write("tests/README.md", "# Tests\n\nThe tests.\n");
    fixture.write(
        "tests/fixtures/README.md",
        "# Fixtures\n\nExecutable examples.\n",
    );
    fixture.write(
        "tests/fixtures/connection/README.md",
        "# Connection Fixture\n\nAn example connection.\n",
    );
    fixture.write("tests/fixtures/connection/input.md", "# Fixture Input\n");
    fixture.write("qa/fixtures/example/README.md", "# Fixture Data\n");
    fixture.generate();
    assert!(
        fixture
            .read("tests/README.md")
            .contains("fixtures/README.md")
    );
    let hub = fixture.read("tests/fixtures/README.md");
    assert!(hub.contains("[Connection Fixture](connection/README.md): An example connection"));
    assert!(!hub.contains("input.md"));
    assert!(!fixture.read("README.md").contains("qa/fixtures"));
    assert!(Navigation::new(&fixture.tree()).unreachable().is_empty());
}

#[test]
fn missing_child_and_sibling_pages_are_found_and_repaired() {
    let fixture = Fixture::new("child-sibling");
    fixture.write("README.md", "# Root\n\nThe product.\n");
    fixture.write("guide.md", "# Guide\n\nLearn to use it.\n");
    fixture.write("child/README.md", "# Child\n\nChild feature.\n");
    fixture.write("child/details.md", "# Details\n\nChild internals.\n");
    let before = fixture.tree();
    assert_eq!(check(&before).len(), 2);
    assert_eq!(reachability(&before).len(), 3);
    fixture.generate();
    let root = fixture.read("README.md");
    assert!(root.contains("## Docs\n\n- "));
    assert!(!root.contains("Use the links below"));
    assert!(root.contains("[Guide](guide.md): Learn to use it"));
    assert!(root.contains("[Child](child/README.md): Child feature"));
    assert!(
        !root.contains("child/details.md"),
        "must not skip a child hub"
    );
    let after = fixture.tree();
    assert!(check(&after).is_empty());
    assert!(reachability(&after).is_empty());
}

#[test]
fn docs_bridge_folders_need_no_synthetic_readmes() {
    let fixture = Fixture::new("bridge");
    fixture.write("README.md", "# Root\n\nRepository.\n");
    fixture.write("package/README.md", "# Package\n\nPackage overview.\n");
    fixture.write(
        "package/docs/feature/README.md",
        "# Feature\n\nWhat the feature does.\n",
    );
    fixture.write(
        "package/docs/feature/reference.md",
        "# Reference\n\nExact interface.\n",
    );
    fixture.write(
        "docs/releases/alpha/plan.md",
        "# Plan\n\nPrivate release plan.\n",
    );
    fixture.generate();
    assert!(!fixture.root.join("package/docs/README.md").exists());
    assert!(!fixture.root.join("package/docs/README.md").exists());
    assert!(
        fixture
            .read("package/README.md")
            .contains("docs/feature/README.md")
    );
    assert!(
        fixture
            .read("README.md")
            .contains("docs/releases/alpha/plan.md")
    );
    assert!(Navigation::new(&fixture.tree()).unreachable().is_empty());
}

#[test]
fn indexes_are_stable_and_preserve_prose_code_and_editorial_links() {
    let fixture = Fixture::new("preserve");
    fixture.write("README.md", "# Root\n\nHandwritten opening.\n\n## Docs\n\nHandwritten guidance stays here.\n\n- [Old name](guide.md): old words\n- [External](https://example.com): useful context\n\n## Code\n\n- [`lib.rs`](lib.rs): main code\n\n## Contributing\n\nMore handwritten prose.\n");
    fixture.write(
        "guide.md",
        "# Guide\n\nAn up-to-date description. More detail.\n",
    );
    fixture.generate();
    let first = fixture.read("README.md");
    assert!(!first.contains("Old name"));
    assert!(first.contains("Handwritten guidance stays here."));
    assert!(first.contains("[External](https://example.com): useful context"));
    assert!(first.contains("## Code\n\n- [`lib.rs`](lib.rs): main code"));
    assert!(first.ends_with("## Contributing\n\nMore handwritten prose.\n"));
    assert_eq!(first.matches(START).count(), 1);
    assert_eq!(first.matches(END).count(), 1);
    assert!(
        Navigation::new(&fixture.tree())
            .updates()
            .unwrap()
            .is_empty()
    );
    fixture.generate();
    assert_eq!(fixture.read("README.md"), first);
    fixture.write("guide.md", "# Renamed Guide\n\nChanged description.\n");
    assert_eq!(check(&fixture.tree()).len(), 1);
    fixture.generate();
    assert!(
        fixture
            .read("README.md")
            .contains("[Renamed Guide](guide.md): Changed description")
    );
}

#[test]
fn fixture_source_internal_guides_and_translations_are_excluded() {
    let fixture = Fixture::new("exclusions");
    fixture.write("README.md", "# Root\n\nRepository.\n");
    fixture.write(
        "package/Cargo.toml",
        "[package]\nname = 'example'\ndescription = 'Package description'\n",
    );
    fixture.write("package/README.md", "# Package\n\nOpening paragraph.\n");
    fixture.write("package/docs/page.md", "# English\n\nEnglish page.\n");
    for path in [
        "AGENTS.md",
        "CLAUDE.md",
        concat!("HUMAN", ".md"),
        ".agents/skills/test/SKILL.md",
        "package/src/README.md",
        "package/tests/README.md",
        "fixtures/README.md",
        "assets/license.md",
        "package/docs/page.es.md",
        "package/docs/page.pt-BR.md",
        "package/docs/website.md",
    ] {
        fixture.write(path, "# Ignored\n\nNot a reading-tree page.\n");
    }
    fixture.write("docs/checks/case.md", "# Checks\n\nA quality check.\n");
    assert_eq!(Navigation::new(&fixture.tree()).document_count(), 4);
    fixture.generate();
    assert!(
        fixture
            .read("README.md")
            .contains("[Package](package/README.md): Package description")
    );
    assert!(reachability(&fixture.tree()).is_empty());
}

#[test]
fn orphan_subtrees_and_broken_reading_links_fail_even_with_markers() {
    let fixture = Fixture::new("orphan");
    fixture.write("README.md", "# Root\n\nRepository.\n");
    fixture.write(
        concat!("docs/", "plans/README.md"),
        "# Plans\n\nPlans live here.\n",
    );
    fixture.write(concat!("docs/", "plans/detail.md"), "# Detail\n\nA plan.\n");
    fixture.generate();
    let root = fixture.read("README.md");
    fixture.write(
        "README.md",
        &root.replace(concat!("docs/", "plans/README.md"), "docs/missing.md"),
    );
    let tree = fixture.tree();
    assert_eq!(Navigation::new(&tree).unreachable().len(), 2);
    assert_eq!(check(&tree).len(), 1);
    fixture.generate();
    assert!(reachability(&fixture.tree()).is_empty());
    std::fs::remove_file(fixture.root.join("README.md")).unwrap();
    assert_eq!(Navigation::new(&fixture.tree()).unreachable().len(), 2);
}

#[test]
fn malformed_markers_refuse_to_overwrite_authored_content() {
    let fixture = Fixture::new("malformed");
    let text = format!("# Root\n\nRepository.\n\n{START}\nUnfinished authored content.\n");
    fixture.write("README.md", &text);
    assert!(Navigation::new(&fixture.tree()).updates().is_err());
    assert_eq!(fixture.read("README.md"), text);
}

#[test]
fn multiline_migration_preserves_other_links_and_inline_code() {
    let fixture = Fixture::new("multiline");
    fixture.write("README.md", "# Root\n\nRepository.\n\n## Docs\n\n- [Old\n  title](guide.md): old\n  description\n- [External\n  reference](https://example.com): keep this\n\n## Code\n\n- [`lib.rs`](lib.rs): code\n");
    fixture.write(
        "guide.md",
        "# Guide\n\nRun `lnk up` and edit `config.toml`.\n",
    );
    fixture.generate();
    let root = fixture.read("README.md");
    assert!(!root.contains("Old\n"));
    assert!(!root.contains("  description"));
    assert!(root.contains("External\n  reference"));
    assert!(root.contains("Run `lnk up` and edit `config.toml`"));
    assert!(
        Navigation::new(&fixture.tree())
            .updates()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn encoded_paths_are_followed_by_navigation_and_link_checks() {
    let fixture = Fixture::new("encoded-paths");
    fixture.write("README.md", "# Root\n\nRepository.\n");
    fixture.write(
        "docs/a (b)% #?.md",
        "# Encoded\n\nA path with reserved characters.\n",
    );
    fixture.generate();
    let root = fixture.read("README.md");
    assert!(root.contains("a%20%28b%29%25%20%23%3F.md"));
    let mut tree = fixture.tree();
    tree.folders.push(fixture.root.clone());
    assert!(Navigation::new(&tree).unreachable().is_empty());
    let rules: Vec<_> = link_docs_check::rules()
        .into_iter()
        .filter(|r| r.id == "links")
        .collect();
    link_docs_check::run(&tree, &rules).assert();
}

#[test]
fn generated_navigation_keeps_authored_size_and_heading_budgets() {
    let fixture = Fixture::new("authored-budgets");
    fixture.write("README.md", "# Root\n\nRepository.\n");
    let mut page = String::from("# Guide\n\nAn authored introduction.\n");
    for number in 0..10 {
        page += &format!("\n## Topic {number}\n\nAuthored topic.\n");
    }
    while page.lines().count() < 499 {
        page += "Authored line.\n";
    }
    fixture.write("docs/README.md", &page);
    for number in 0..30 {
        fixture.write(
            &format!("docs/page-{number}.md"),
            "# Page\n\nA child page.\n",
        );
    }
    fixture.generate();
    assert!(fixture.read("docs/README.md").lines().count() > 500);
    let mut tree = fixture.tree();
    tree.folders.push(fixture.root.clone());
    let budget_rules: Vec<_> = link_docs_check::rules()
        .into_iter()
        .filter(|r| matches!(r.id, "markdown-lines" | "headings"))
        .collect();
    link_docs_check::run(&tree, &budget_rules).assert();
    let rendered = fixture.read("docs/README.md");
    fixture.write(
        "docs/README.md",
        &format!("{rendered}\n## Extra\n\nAnother topic.\n"),
    );
    let mut tree = fixture.tree();
    tree.folders.push(fixture.root.clone());
    let report = link_docs_check::run(&tree, &budget_rules);
    assert!(report.failures.contains_key("markdown-lines"));
    assert!(report.failures.contains_key("headings"));
}

#[test]
fn descriptions_preserve_code_literals_while_removing_prose_markup() {
    let fixture = Fixture::new("code-descriptions");
    fixture.write("README.md", "# Root\n\nRepository.\n");
    fixture.write(
        "guide.md",
        "# Guide\n\nRead **files** matching `**/*.md` and literal `[name](path)` with [the guide](other.md).\n",
    );
    fixture.generate();
    let root = fixture.read("README.md");
    assert!(
        root.contains("Read files matching `**/*.md` and literal `[name](path)` with the guide")
    );
    assert!(
        Navigation::new(&fixture.tree())
            .updates()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn generated_indexes_have_clean_endings_and_allow_missing_descriptions() {
    let fixture = Fixture::new("whitespace");
    fixture.write("README.md", "# Root\n\nRepository.\n\n");
    fixture.write("guide.md", "# Guide\n");
    fixture.generate();
    let root = fixture.read("README.md");
    assert!(root.contains("- [Guide](guide.md)\n"));
    assert!(!root.ends_with("\n\n"));
    assert!(root.lines().all(|line| line == line.trim_end()));
    assert!(
        Navigation::new(&fixture.tree())
            .updates()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn package_runtime_markdown_is_not_documentation() {
    let fixture = Fixture::new("runtime-markdown");
    fixture.write("README.md", "# Root\n\nRepository documentation.\n");
    fixture.write("package/Cargo.toml", "[package]\nname = 'example'\n");
    fixture.write("package/README.md", "# Package\n\nPackage documentation.\n");
    for name in ["init", "learn", "review"] {
        fixture.write(
            &format!("package/commands/{name}.md"),
            "Runtime instructions consumed by the program.\n",
        );
    }
    fixture.write("package/prompts/base.md", "Runtime system prompt.\n");
    fixture.write(
        "package/docs/commands/README.md",
        "# Commands\n\nHow users invoke commands.\n",
    );
    fixture.write(
        "package/docs/commands/review.md",
        "# Review\n\nThe documented review command.\n",
    );
    assert_eq!(Navigation::new(&fixture.tree()).document_count(), 4);
    fixture.generate();
    let hub = fixture.read("package/README.md");
    assert!(hub.contains("docs/commands/README.md"));
    for name in ["init", "learn", "review"] {
        assert!(!hub.contains(&format!("](commands/{name}.md)")));
        assert_eq!(
            fixture.read(&format!("package/commands/{name}.md")),
            "Runtime instructions consumed by the program.\n"
        );
    }
    assert!(!hub.contains("prompts/base.md"));
    assert!(
        fixture
            .read("package/docs/commands/README.md")
            .contains("[Review](review.md)")
    );
    let tree = fixture.tree();
    assert!(check(&tree).is_empty());
    assert!(reachability(&tree).is_empty());
}

#[test]
fn indexes_are_alphabetical_and_hubs_head_them_docs() {
    let fixture = Fixture::new("kinds");
    fixture.write("README.md", "# Root\n\nThe product.\n");
    fixture.write("zeta.md", "# Zeta\n\nThe last page.\n");
    fixture.write(
        "group/README.md",
        &format!("# Group\n\nCrates.\n\n{START}\n\n{END}\n"),
    );
    fixture.write("group/kit/README.md", "# Kit\n\nA library.\n");
    fixture.write("group/kit/Cargo.toml", "[package]\nname = \"kit\"\n");
    fixture.write("group/kit/part/README.md", "# Part\n\nA part.\n");
    fixture.write("group/kit/part/Cargo.toml", "[package]\nname = \"part\"\n");
    fixture.write(
        "pages/README.md",
        &format!("# Pages\n\nOnly pages.\n\n{START}\n\n{END}\n"),
    );
    fixture.generate();
    assert!(fixture.read("README.md").contains("- [Group](group/README.md): Crates\n- [Pages](pages/README.md): Only pages\n- [Zeta](zeta.md): The last page\n"));
    assert!(
        fixture
            .read("group/README.md")
            .contains("## Docs\n\n- [Kit](kit/README.md): A library\n")
    );
    assert!(fixture.read("group/kit/README.md").contains("## Docs\n"));
    assert_eq!(fixture.read("pages/README.md"), "# Pages\n\nOnly pages.\n");
    assert!(check(&fixture.tree()).is_empty());
}

#[test]
fn code_indexes_sort_children_by_kind_and_open_their_own_docs() {
    let fixture = Fixture::new("sections");
    fixture.write("README.md", "# Root\n\nThe product.\n");
    fixture.write(
        "group/README.md",
        &format!("# Group\n\nCrates.\n\n{START}\n\n{END}\n"),
    );
    fixture.write("group/docs/group/README.md", "# Guide\n\nThe group.\n");
    fixture.write("group/docs/group/zeta.md", "# Zeta\n\nLast.\n");
    fixture.write("group/docs/group/alpha.md", "# Alpha\n\nFirst.\n");
    fixture.write("group/kit/README.md", "# Kit\n\nA library.\n");
    fixture.write("group/kit/Cargo.toml", "[package]\nname = \"kit\"\n");
    fixture.write("group/tool/README.md", "# Tool\n\nA module.\n");
    fixture.write(
        "group/tool/Cargo.toml",
        "[package]\nname = \"tool\"\n\n[[bin]]\nname = \"lnk-tool\"\n",
    );
    // A module is what its `module.toml` declares; a program named
    // `lnk-<name>` without one, a fixture's or a tool's, is a package.
    fixture.write(
        "group/tool/module.toml",
        "schema = 1\n[module]\nid = \"tool\"\nkind = \"module\"\nowner = \"group/tool\"\n",
    );
    fixture.write("group/report/README.md", "# Report\n\nA tool.\n");
    fixture.write(
        "group/report/Cargo.toml",
        "[package]\nname = \"report\"\n\n[[bin]]\nname = \"lnk-report\"\n",
    );
    fixture.generate();
    let group = fixture.read("group/README.md");
    let expected = "## Docs\n\n### Pages\n\n\
        - [Guide](docs/group/README.md): The group\n  \
        - [Alpha](docs/group/alpha.md): First\n  \
        - [Zeta](docs/group/zeta.md): Last\n\n\
        ### Modules\n\n- [Tool](tool/README.md): A module\n\n\
        ### Packages\n\n- [Kit](kit/README.md): A library\n- [Report](report/README.md): A tool\n";
    assert!(group.contains(expected), "{group}");
    // A page index stays one flat list.
    assert!(
        fixture
            .read("group/docs/group/README.md")
            .contains("## Docs\n\n- [Alpha](alpha.md): First\n- [Zeta](zeta.md): Last\n")
    );
    assert!(check(&fixture.tree()).is_empty());
}

#[test]
fn declared_whole_data_is_not_current_navigation_but_links_and_line_checks_keep_it() {
    let fixture = Fixture::new("whole-data");
    fixture.write(
        "large-files.toml",
        "data=['records','docs/records']\n[lines]\n",
    );
    fixture.write(
        "scripts/documentation-index.json",
        r#"{"sites":[],"roots":[{"directory":"."}]}"#,
    );
    fixture.write(
        "README.md",
        "# Root\n\n[Map](map.md). [Record](records/run/session.md#frozen).\n",
    );
    fixture.write("map.md", "# Map\n\nThe current map.\n");
    fixture.write(
        "records/README.md",
        "# Frozen Records\n\n[Old](removed.md).\n",
    );
    let record = format!(
        "# Frozen\n\n[Historical](gone.md).\n{}",
        "entry\n".repeat(501)
    );
    fixture.write("records/run/session.md", &record);
    fixture.write("docs/records/item.md", "# Input\n\n[Old](gone.md).\n");
    fixture.write(
        "records-current/README.md",
        "# Current Records Guide\n\nThe current guide.\n",
    );
    fixture.write("records-current/guide.md", "# Guide\n\nCurrent guidance.\n");
    fixture.write("docs/current.md", "# Current\n\nCurrent documentation.\n");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        "../../records/run/session.md",
        fixture.root.join("docs/records/linked.md"),
    )
    .unwrap();
    let tree = fixture.tree();
    assert_eq!(
        tree.text(&fixture.root.join("records/run/session.md")),
        Some(record.as_str())
    );
    assert!(!tree.is_doc(&fixture.root.join("docs/records/item.md")));
    assert!(tree.is_doc(&fixture.root.join("docs/current.md")));
    #[cfg(unix)]
    assert_eq!(
        tree.text(&fixture.root.join("docs/records/linked.md")),
        Some(record.as_str())
    );
    let updates = Navigation::new(&tree).updates().unwrap();
    assert!(
        updates
            .iter()
            .all(|u| !u.path.starts_with(fixture.root.join("records")))
    );
    fixture.generate();
    assert_eq!(fixture.read("records/run/session.md"), record);
    assert_eq!(
        fixture.read("records/README.md"),
        "# Frozen Records\n\n[Old](removed.md).\n"
    );
    assert!(
        fixture
            .read("records-current/README.md")
            .contains("[Guide](guide.md)")
    );
    assert!(check(&fixture.tree()).is_empty());
    let rules = link_docs_check::rules();
    let links: Vec<_> = rules.iter().copied().filter(|r| r.id == "links").collect();
    // Frozen outgoing links are not current promises; an incoming link to
    // a real record still resolves against its retained contents/heading.
    link_docs_check::run(&fixture.tree(), &links).assert();
    fixture.write("map.md", "# Map\n\n[Missing](missing-map.md).\n");
    fixture.write(
        "records-current/guide.md",
        "# Guide\n\n[Missing](missing-current.md).\n",
    );
    fixture.write(
        "docs/current.md",
        "# Current\n\n[Missing](missing-doc.md).\n",
    );
    let broken = link_docs_check::run(&fixture.tree(), &links);
    let failures = &broken.failures["links"];
    assert_eq!(failures.len(), 3, "{failures:?}");
    for page in ["map.md", "records-current/guide.md", "docs/current.md"] {
        assert!(
            failures.iter().any(|f| f.starts_with(&format!("{page}:"))),
            "{failures:?}"
        );
    }
    fixture.write(
        "README.md",
        "# Root\n\n[Missing Record](records/run/missing.md).\n",
    );
    let broken = link_docs_check::run(&fixture.tree(), &links);
    assert!(
        broken.failures["links"]
            .iter()
            .any(|f| f.contains("records/run/missing.md (no such file)"))
    );
    fixture.write(
        "README.md",
        "# Root\n\n[Wrong Heading](records/run/session.md#missing).\n",
    );
    let broken = link_docs_check::run(&fixture.tree(), &links);
    assert!(
        broken.failures["links"]
            .iter()
            .any(|f| f.contains("session.md#missing (no such heading)"))
    );
    let lines: Vec<_> = rules
        .iter()
        .copied()
        .filter(|r| r.id == "markdown-lines")
        .collect();
    let lines = link_docs_check::run(&fixture.tree(), &lines);
    assert!(
        lines.failures["markdown-lines"]
            .iter()
            .any(|f| f.starts_with("records/run/session.md:"))
    );
}
