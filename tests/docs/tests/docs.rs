//! The public workspace follows every rule of its docs, and the checks
//! find what they look for: each rule fails on a workspace made to break
//! it, and the helpers read Markdown as the docs write it.

link_test_kit::program!(unit);

use std::path::Path;

use link_docs_check::md::{commands, links, slug, spans, words};
use link_docs_check::{Tree, rules, run};

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn here() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn page_metadata_and_card_delimiters_are_not_prose() {
    use link_docs_check::prose::{Block, blocks};
    let source = "---\ntitle: Page\ndescription: Metadata\n---\n\n# Page\n\nIntroduction.\n\n:::cards\n- [Guide](guide.md)\n- [Other](other.md)\n:::\n";
    assert_eq!(
        blocks(source),
        vec![
            Block::Heading(1, "Page".into()),
            Block::Paragraph("Introduction.".into()),
            Block::Other,
            Block::List(vec!["[Guide](guide.md)".into(), "[Other](other.md)".into()]),
            Block::Other,
        ]
    );
    let prose = link_docs_check::md::split(source).0;
    assert!(!prose.iter().any(|line| line.starts_with("title:")));
    assert_eq!(links(&prose), ["guide.md", "other.md"]);
    let incomplete = "---\n# Page\n\nVisible.";
    assert_eq!(link_docs_check::md::body(incomplete), incomplete);
    assert_eq!(
        link_docs_check::md::body("---\r\ntitle: Café\r\n---\r\n# Page"),
        "# Page"
    );
}

/// A rule that panics fails with where it panicked, and every other rule
/// still reports: one run lists everything to fix.
#[test]
fn a_panicking_rule_hides_no_other_rules_findings() {
    use link_docs_check::{Check, Finding, Rule, Scope, fail};
    fn panics(_: &Tree) -> Vec<Finding> {
        panic!("a rule's own bug")
    }
    fn finds(_: &Tree) -> Vec<Finding> {
        vec![fail("something to fix")]
    }
    let dir = link_test_kit::TempDir::new("panicking-rule");
    write(dir.path(), "README.md", "# Probe\n");
    let tree = Tree::read(dir.path(), false, &here().join("large-files.toml"));
    let rule = |id, check| Rule {
        id,
        scope: Scope::Every,
        fix: "fix it",
        check: Check::Whole(check),
    };
    let report = run(&tree, &[rule("panics", panics), rule("finds", finds)]);
    assert_eq!(report.failures["finds"], ["something to fix"]);
    let panicked = &report.failures["panics"];
    assert!(
        panicked[0].contains("panicked: a rule's own bug"),
        "{panicked:?}"
    );
}

#[test]
fn the_docs_follow_every_rule() {
    let tree = Tree::read(
        &here().join("../.."),
        true,
        &here().join("large-files.toml"),
    );
    run(&tree, &rules()).assert();
}

#[test]
fn the_checkout_has_one_complete_reading_tree() {
    let workspace = Tree::read(
        &here().join("../.."),
        false,
        &here().join("large-files.toml"),
    );
    let checkout = Tree::read(&workspace.checkout, false, &here().join("large-files.toml"));
    run(&checkout, link_docs_check::navigation::RULES).assert();
}

#[test]
fn every_rule_fails_where_it_is_broken() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("docs-rules");
    let _ = std::fs::remove_dir_all(&root);
    write(&root, ".github/README.md", "# Hub\n");
    write(
        &root,
        "scripts/documentation-index.json",
        r#"{"sites": [{"url": "https://www.local.link", "directory": "web/apps/first", "roots": true}],
            "roots": [{"directory": "a"}, {"directory": "b"}, {"directory": "d"}, {"directory": "d/inner"}]}"#,
    );
    let tabs: String = (0..11)
        .map(|n| format!("- [P{n}](https://www.local.link/p{n})\n"))
        .collect();
    write(
        &root,
        "web/apps/first/docs/navigation.md",
        &format!(
            "# Website\n\n## Pages\n\n{tabs}- [Guide](../../../../b/docs/guide.md)\n  - [Other](../../../../b/docs/page.md)\n"
        ),
    );
    // A guide holding a page, the only one under it.
    write(
        &root,
        "b/docs/guide.md",
        "# Guide\n\nWhere a reader starts.\n",
    );
    // A module with commands of its own, but no guide or troubleshooting.
    write(
        &root,
        "b/docs/more/reference/modules/b.md",
        &format!(
            "<!-- {} -->\n# B Commands\n",
            link_actions_surfaces::marker()
        ),
    );
    write(&root, "web/apps/first/src/words.md", "Words.\n");
    write(
        &root,
        "web/apps/first/docs/about.md",
        "# About Us\n\n> About the people here\n\nWho we are, and what we make.\n",
    );
    write(&root, "a/docs/page.es.md", "# Empezar\n\nSin su inglés.\n");
    let crowded: String = (0..11).map(|n| format!("## H{n}\n\n")).collect();
    let private = concat!("docs/", "plans");
    write(
        &root,
        "a/docs/page.md",
        &format!(
            "# Get Started\n\nWe tried another way.\n\n[gone](gone.md) names {private}.\n\n\
             ```sh\ncurl x | sh   # a note\nlnk agent show   # is it up?\nKEY=x lnk agent start   # with a key\n```\n\n\
             ## Layout\n\n- `unshipped/<domain>/`: not present here.\n\n\
             ## Decisions\n\n{crowded}"
        ),
    );
    write(&root, "b/docs/page.md", "# Other\n");
    write(&root, "b/docs/lower.md", "# A lower title\n\nText.\n");
    write(&root, "b/docs/probe.py", "print()\n");
    write(&root, "b/module.toml", "schema = 1\n");
    write(&root, "b/docs/README.md", "# B\n\nB holds its module.\n");
    write(
        &root,
        "b/docs/bare.md",
        "# Bare\n\nRun lnk agent start with LNK_HOME set to ~/.lnk/home.\n\n## Thin\n\nNone.\n\n## Troubleshooting\n\nWhat to do when it breaks, one symptom at a time, each with its fix.\n",
    );
    write(
        &root,
        "b/docs/hooked.md",
        &format!("# Hooked\n\n> {}\n\nText.\n", "hook ".repeat(10)),
    );
    write(
        &root,
        "b/docs/long.md",
        &format!(
            "# Other\n\n{}\n\n| {} |\n|---|\n\n- {}\n\nKeys.\n\n| Key |\n|---|\n| `a` |\n| `b` |\n\nRows.\n\n| Row |\n|---|\n| A one |\n| The other |\n",
            "word ".repeat(121),
            "cell ".repeat(13),
            "item ".repeat(81)
        ),
    );
    write(&root, "a/docs/fr/page.md", "# Page\n");
    write(
        &root,
        "a/README.md",
        &format!("# A\n\n{}\n\n[deep](c/d/x.md)\n", "word ".repeat(121)),
    );
    write(
        &root,
        "a/c/README.md",
        "# C\n\n- [x](d/x.md)\n\n## Code\n\n- [x](d/x.md)\n\n## Docs\n",
    );
    write(&root, "a/c/d/x.md", "# X\n");
    write(
        &root,
        "a/Cargo.toml",
        "[package]\nname = \"a\"\n\n[lib]\ndoctest = false\n",
    );
    write(
        &root,
        "a/src/lib.rs",
        "//! A.\n//!\n//! ```\n//! a::run();\n//! ```\n",
    );
    write(&root, "b/Cargo.toml", "[package]\nname = \"b\"\n");
    write(&root, "b/src/lib.rs", "//! B.\n");
    write(&root, "a/big.rs", &"//\n".repeat(1001));
    write(&root, "a/big.md", &"-\n".repeat(501));
    write(
        &root,
        "large-files.toml",
        "unshaped = [\"gone.md\"]\nundocumented = [\"gone\"]\n\n[held]\nintroduced = [\"gone.md\"]\n\n[lines]\n\"gone.rs\" = 2000\n",
    );
    write(
        &root,
        "modules/a/src/main.rs",
        "fn main() { println!(\"hi\"); }\n",
    );
    write(&root, "packages/layer.toml", "depends_on = [\"core\"]\n");

    write(
        &root,
        "b/docs/abbreviated.md",
        "# Abbreviated\n\nIt speaks ACP before the Agent Client Protocol (ACP).\n",
    );
    write(
        &root,
        "b/docs/aside.md",
        "# Aside\n\nIt runs on its own (see [Other](page.md)).\n",
    );
    write(
        &root,
        "b/docs/rusty.md",
        "# Rusty\n\nIt's kept by `store.rs`.\n",
    );
    let said = format!("{}.", "the same words said on two pages ".repeat(5));
    write(&root, "b/docs/one.md", &format!("# One\n\n{said}\n"));
    write(&root, "b/docs/two.md", &format!("# Two\n\n{said}\n"));
    write(
        &root,
        "b/docs/cloud.md",
        "# Cloud\n\nIt runs on a host machine.\n",
    );
    write(&root, "b/docs/short.md", "# Short\n\n> Short\n\nText.\n");
    write(
        &root,
        "b/docs/vague.md",
        "# What It Does\n\nThe page's subject, unnamed.\n",
    );
    write(
        &root,
        "b/docs/out.md",
        "# Out\n\nIt bundles.\n\n```sh\nlnk bundle create devbox --out ~/dev-bundle\n```\n",
    );
    // A terminal block writing its file with a heredoc, and a file's block
    // with a comment beyond its path.
    write(
        &root,
        "b/docs/heredoc.md",
        "# Heredoc\n\nIt checks a file.\n\n```sh\ncat > env.toml <<'EOF'\nname = \"app\"\nEOF\nlnk env check --spec env.toml\n```\n\nThe file.\n\n```toml\n# env.toml\nname = \"app\" # its name\n```\n",
    );
    write(&root, "b/docs/skip.md", "# Skip\n\n### Deep\n\nText.\n");
    write(
        &root,
        "b/docs/s/t/u/v/w/x/deep.md",
        "# Deep\n\n## Deeper\n\nText.\n",
    );
    write(&root, "c/Cargo.toml", "[package]\nname = \"c\"\n");
    write(
        &root,
        "c/src/lib.rs",
        &(0..900)
            .map(|n| format!("pub const C{n}: usize = {n};\n"))
            .collect::<String>(),
    );
    write(&root, "d/Cargo.toml", "[package]\nname = \"d\"\n");
    write(&root, "d/docs/d/README.md", "# D\n\nD.\n");
    write(&root, "d/inner/Cargo.toml", "[package]\nname = \"inner\"\n");
    write(
        &root,
        "d/inner/docs/elsewhere.md",
        "# Elsewhere\n\nAway from d.\n",
    );
    write(&root, "wrapper/README.md", "# Wrapper\n\nOne group.\n");
    write(&root, "wrapper/only/README.md", "# Only\n\nOne child.\n");
    // A module whose command's verb has no page of its own.
    write(
        &root,
        "d/actions.toml",
        "[d]\ndispatch = true\nrole = \"secondary\"\n[\"d make\"]\nrole = \"primary\"\n",
    );
    // A chain six links deep from the root README, past both click limits.
    write(&root, "README.md", "# Root\n\n[One](deep/1.md).\n");
    for n in 1..=6 {
        write(
            &root,
            &format!("deep/{n}.md"),
            &format!("# Page {n}\n\nOn to [the next]({}.md).\n", n + 1),
        );
    }

    write(&root, "Cargo.toml", "[workspace]\nmembers = [\"m\"]\n");
    write(
        &root,
        "m/module.toml",
        "schema = 1\n[module]\nid = \"m\"\nkind = \"module\"\nowner = \"modules/m\"\n",
    );
    write(
        &root,
        "m/types.toml",
        "schema = 1\n[m]\ntype = \"object\"\nx-link-spec = { kind = \"m\" }\nproperties.name = { type = \"string\" }\n",
    );
    // An IAM page saying a word The IAM Model retired.
    write(&root, "iam-pages.toml", "folders = [\"iam\"]\n");
    write(&root, "iam/README.md", "# IAM\n\nA customer signs in.\n");
    let tree = Tree::read(&root, true, &root.join("large-files.toml"));
    assert_eq!(
        tree.site_page("https://www.local.link/agents/agent#x")
            .map(|(_, address)| address),
        Some("https://www.local.link/agents/agent")
    );
    assert!(tree.site_page("https://local.link/install.sh").is_none());
    let report = run(&tree, &rules());
    let missed: Vec<&str> = rules()
        .iter()
        .map(|rule| rule.id)
        .filter(|id| {
            if *id == "single-child-groups" || *id == "verb-pages" {
                !report.warnings.contains_key(id)
            } else {
                !report.failures.contains_key(id)
            }
        })
        .collect();
    assert!(
        missed.is_empty(),
        "these rules found nothing: {missed:?}\n{:#?}",
        report.failures
    );
}

#[test]
fn workspace_readme_links_to_feature_docs_without_skipping_source_hubs() {
    let root = link_test_kit::temp::TempDir::new("readme-feature-docs");
    for (path, text) in [
        (
            "README.md",
            "# Product\n\n[Agents](modules/agents/docs/agents/README.md).\n",
        ),
        ("modules/README.md", "# Modules\n\nThe modules.\n"),
        ("modules/agents/README.md", "# Agents\n\nAn agent module.\n"),
        (
            "modules/agents/docs/agents/README.md",
            "# Agents\n\nStart an agent.\n",
        ),
        ("modules/agents/src/main.rs", "fn main() {}\n"),
        ("large-files.toml", "[lines]\n"),
    ] {
        write(&root, path, text);
    }
    let rule: Vec<_> = rules()
        .into_iter()
        .filter(|rule| rule.id == "through-readmes")
        .collect();
    let tree = || Tree::read(&root, false, &root.join("large-files.toml"));
    run(&tree(), &rule).assert();

    write(
        &root,
        "README.md",
        "# Product\n\n[Source](modules/agents/src/main.rs).\n",
    );
    write(
        &root,
        "modules/README.md",
        "# Modules\n\n[Agent Guide](agents/docs/agents/README.md).\n",
    );
    let report = run(&tree(), &rule);
    let failures = &report.failures["through-readmes"];
    assert_eq!(failures.len(), 2, "{failures:?}");
    assert!(
        failures
            .iter()
            .any(|f| f.contains("modules/agents/src/main.rs"))
    );
    assert!(failures.iter().any(|f| f.starts_with("modules/README.md:")));
}

#[test]
fn workspace_landing_readme_is_not_limited_like_folder_maps() {
    let root = link_test_kit::TempDir::new("readme-landing-words");
    let limit = link_docs_check::Limits::read().of("hub-words");
    let text = format!("# Product\n\n{}\n", "word ".repeat(limit + 1));
    write(&root, "README.md", &text);
    write(&root, "modules/README.md", &text);
    write(&root, "large-files.toml", "[lines]\n");
    let tree = Tree::read(&root, false, &root.join("large-files.toml"));
    let rule: Vec<_> = rules()
        .into_iter()
        .filter(|rule| rule.id == "hub-words")
        .collect();
    let report = run(&tree, &rule);
    let failures = &report.failures["hub-words"];
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].starts_with("modules/README.md:"));
}

#[test]
fn a_package_code_list_is_alphabetical_by_name() {
    let root = link_test_kit::TempDir::new("readme-code-order");
    write(&root, "large-files.toml", "[lines]\n");
    write(&root, "a/Cargo.toml", "[package]\nname = \"a\"\n");
    write(
        &root,
        "a/README.md",
        "# A\n\nA.\n\n## Code\n\n- [`b.rs`](b.rs): b\n- [`a.rs`](a.rs): a\n",
    );
    let rule: Vec<_> = rules()
        .into_iter()
        .filter(|r| r.id == "readme-shape")
        .collect();
    let report = run(
        &Tree::read(&root, false, &root.join("large-files.toml")),
        &rule,
    );
    assert_eq!(
        report.failures["readme-shape"],
        ["a/README.md: ## Code isn't alphabetical by name"]
    );
}

/// Rust source is named only where a maintainer reads: a decisions page,
/// a page below a member's `docs/more/` and a folder the workspace's
/// `rust-pages.toml` lists may name it, and every other page may not.
#[test]
fn only_a_page_a_maintainer_reads_names_rust_source() {
    let root = link_test_kit::TempDir::new("rust-names");
    write(&root, "large-files.toml", "[lines]\n");
    write(&root, "rust-pages.toml", "folders = [\"ops\"]\n");
    let rusty = "# Page\n\nIt's kept by `store.rs`.\n";
    for page in [
        "b/docs/guide.md",
        "b/docs/decisions.md",
        "b/docs/more/library.md",
        "b/docs/agents/tools/more/library.md",
        "b/docs/moreover/page.md",
        "b/more/docs/page.md",
        "ops/docs/runbook.md",
        "opsy/docs/runbook.md",
    ] {
        write(&root, page, rusty);
    }
    write(
        &root,
        "b/README.md",
        "# B\n\nB.\n\n## Code\n\n- `src/lib.rs`\n",
    );
    write(&root, "t/docs/Cargo.toml", "[package]\nname = \"t\"\n");
    write(&root, "t/docs/code/README.md", rusty);
    let rule: Vec<_> = rules()
        .into_iter()
        .filter(|r| r.id == "rust-names")
        .collect();
    let report = run(
        &Tree::read(&root, false, &root.join("large-files.toml")),
        &rule,
    );
    assert_eq!(
        report.failures["rust-names"],
        [
            "b/docs/guide.md:3: `store.rs`",
            "b/docs/moreover/page.md:3: `store.rs`",
            "b/more/docs/page.md:3: `store.rs`",
            "opsy/docs/runbook.md:3: `store.rs`",
        ]
    );
}

#[test]
fn repository_layout_omits_absent_source_folders() {
    let root = link_test_kit::TempDir::new("repository-layout");
    write(&root, "large-files.toml", "[lines]\n");
    write(
        &root,
        "scripts/documentation-index.json",
        r#"{"roots":[{"directory":"."},{"directory":"packages/example"}]}"#,
    );
    write(&root, "core/README.md", "# Core\n");
    write(&root, "modules/agents/README.md", "# Agents\n");
    write(&root, "packages/example/src/lib.rs", "//! Example.\n");
    let cases = [
        (
            "## Layout\n\n- `core/`: command.\n- `modules/<domain>/`: capabilities.",
            0,
        ),
        (
            "## Layout\n\n- `unshipped/<domain>/`: the public repository\n  has none, so this folder is absent.",
            1,
        ),
        (
            "## Source Tree\n\n| Folder | Purpose |\n|---|---|\n| `unshipped/` | missing |",
            1,
        ),
        (
            "## Layout\n\n- `modules/<domain>/missing/`: absent nested folder.",
            1,
        ),
        (
            "## Layout\n\n- [`unshipped/`](unshipped/): a linked folder.",
            1,
        ),
        (
            "## Layout\n\n- `src/`: owner source.\n- `../../core/`: sibling component.",
            0,
        ),
        (
            "## Layout\n\n- `../../../outside/`: outside the published tree.",
            1,
        ),
        (
            "## Runtime Locations\n\n- `cache/`: created when the program runs.",
            0,
        ),
        (
            "## Layout\n\n- `~/Link/`: human files.\n- `/etc/lnk/`: system files.",
            0,
        ),
    ];
    for (source, expected) in cases {
        write(&root, "packages/example/docs/layout.md", source);
        let tree = Tree::read(&root, false, &root.join("large-files.toml"));
        let report = run(&tree, link_docs_check::layout::RULES);
        assert_eq!(
            report.failures.get("repository-layout").map_or(0, Vec::len),
            expected,
            "{source}: {:?}",
            report.failures
        );
    }
}

#[test]
fn the_helpers_read_markdown_as_the_docs_write_it() {
    assert_eq!(slug("Your AI, the harness"), "your-ai-the-harness");
    assert_eq!(
        slug("Enable `lnk box create` (AWS)"),
        "enable-lnk-box-create-aws"
    );
    assert_eq!(links(&["see [a](b.md#c) and [d](e)"]), ["b.md#c", "e"]);
    let three = |a: &str, b: &str, c: &str| (a.to_string(), b.to_string(), c.to_string());
    assert_eq!(
        commands("lnk agent start --model.default x   # then lnk agent show"),
        [three("agent", "start", "")]
    );
    assert_eq!(
        commands("echo $(lnk box run aws -- ls) && lnk up"),
        [three("box", "run", "aws"), three("up", "", "")]
    );
    assert_eq!(
        commands("lnk agent harness-permission add browser"),
        [three("agent", "harness-permission", "add")]
    );
    assert!(commands("lnk <group> <command>").is_empty());
    assert_eq!(
        spans("Run `lnk agent\n  start` or ``lnk up``, not `lnk-agent`.\n\n`lnk x"),
        ["lnk agent start", "lnk up"]
    );
    assert_eq!(spans("a `b` then `lnk box new`"), ["lnk box new"]);
    assert_eq!(
        words("# Name\n\n**[www.local.link](https://www.local.link/)**\n\n- [a](b), [c](d)\n"),
        0
    );
    assert_eq!(
        words("| [`x/`](x/README.md) | three more words |\n|---|---|\n"),
        3
    );
}

#[test]
fn a_doc_example_is_a_fence_rustdoc_runs() {
    use link_docs_check::doc_tests::example;
    assert_eq!(
        example("//! A.\n//!\n//! ```\n//! a();\n//! ```\n"),
        Some(3)
    );
    assert_eq!(example("/// ```no_run\n/// a();\n/// ```\n"), Some(1));
    assert_eq!(
        example("/// ```compile_fail,E0603\n/// a();\n/// ```\n"),
        Some(1)
    );
    assert_eq!(example("/// ```text\n/// a -> b\n/// ```\n"), None);
    assert_eq!(example("//! ```sh\n//! lnk up\n//! ```\n"), None);
    assert_eq!(example("/// ```ignore\n/// a();\n/// ```\n"), None);
    assert_eq!(example("// ```\n// a();\n// ```\n"), None);
    assert_eq!(
        example("/// ```text\n/// x\n/// ```\nfn a() {}\n/// ```rust\n/// a();\n/// ```\n"),
        Some(5)
    );
}

#[test]
fn comments_that_break_in_zsh_are_found() {
    use link_docs_check::site::{commented_other, zsh_breaks};
    assert!(commented_other("curl -fsSL x | sh   # lnk alone"));
    assert!(!commented_other("lnk agent start   # asks which model"));
    assert!(!commented_other(
        "ANTHROPIC_API_KEY=sk lnk agent start   # hosted"
    ));
    assert!(!commented_other("./target/debug/lnk --version   # which"));
    assert!(!commented_other("# a note on its own line"));
    assert!(!commented_other("curl https://example.com"));
    assert!(zsh_breaks("lnk agent show   # whether it's up").is_some());
    assert!(zsh_breaks("lnk agent show   # is it up?").is_some());
    assert!(zsh_breaks("lnk sandbox run -- bash   # a shell").is_some());
    assert!(zsh_breaks("lnk agent show   # whether it runs and answers").is_none());
    assert!(zsh_breaks("# a note (on its own line)").is_some());
    assert!(zsh_breaks("# a note on its own line").is_none());
}

#[test]
fn the_history_check_distinguishes_records_from_product_facts() {
    use link_docs_check::site::historical_prose;
    for record in [
        "Measured on Oct 14 2026, the build took 123 s.",
        "Decided by the user, the recommendation taken.",
        "Decided by a maintainer after an experiment.",
        "An extra program was decided against.",
        "We tried sharing mutable build directories.",
        "The build took three minutes on 2026-10-14.",
        "The build took three minutes on 14 October 2026.",
        "An experiment on Oct 14 2026 ran until it completed.",
        "The program used to run in a separate package.",
        "We rejected an extra program.",
    ] {
        assert!(historical_prose(record), "{record}");
    }
    for fact in [
        "Each checkout owns its build directory.",
        "May contain user-provided dates.",
        "Effective date: October 10, 2026.",
        "Last updated October 10, 2026.",
        "Old tokens work until Dec 1 2026, then sign in again.",
        "A credential expires on 2026-12-01.",
        "A message says `[Thursday 8 October 2026, 14:03]`.",
        "Report the measured one's memory use.",
        "A credential is a secret used to authorize.",
        "A copied token cannot be used to connect.",
    ] {
        assert!(!historical_prose(fact), "{fact}");
    }
}

#[test]
fn the_tabs_count_pages_under_each() {
    let site = "## Pages\n\n- [A](a)\n  - [B](b)\n  - [C](c)\n    - [D](d)\n- [E](e)\n\n## Next\n";
    assert_eq!(
        link_docs_check::site::crowded(site, &|_| 1),
        [("[A](a)".to_string(), 2), ("(top)".to_string(), 2)]
    );
    assert!(link_docs_check::site::crowded(site, &|_| 2).is_empty());
}

#[test]
fn a_page_excepted_holds_its_own_number_of_pages() {
    let site = "## Pages\n\n- [A](a)\n  - [B](b)\n  - [C](c)\n  - [D](d)\n- [E](e)\n  - [F](f)\n  - [G](g)\n  - [H](h)\n\n## Next\n";
    let roomier = |target: &str| if target == "a" { 3 } else { 2 };
    assert_eq!(
        link_docs_check::site::crowded(site, &roomier),
        [("[E](e)".to_string(), 3)]
    );
    let tighter = |target: &str| if target == "a" { 2 } else { 3 };
    assert_eq!(
        link_docs_check::site::crowded(site, &tighter),
        [("[A](a)".to_string(), 3)]
    );
}

#[test]
fn an_exception_raises_its_rule_for_one_address_and_says_why() {
    let exception = |address: &str, at_most: usize, why: &str| link_docs_check::Exception {
        address: address.to_owned(),
        at_most,
        why: why.to_owned(),
    };
    let limit = |except| link_docs_check::Limit {
        at_most: 10.0,
        exponent: None,
        of: "Pages under a page in the site's tree".to_owned(),
        why: "Group a longer list in a folder.".to_owned(),
        except,
    };
    let check = link_docs_check::check_exceptions;
    let good = limit(vec![exception("/agents", 12, "Room.")]);
    assert_eq!(check("pages", &good), Ok(()));
    assert!(
        check("headings", &good)
            .unwrap_err()
            .contains("no rule reads")
    );
    for (bad, says) in [
        (exception("agents", 12, "Room."), "names a page's address"),
        (exception("/agents/", 12, "Room."), "names a page's address"),
        (exception("/agents", 10, "Room."), "raises the limit"),
        (exception("/agents", 12, " "), "says why"),
    ] {
        let error = check("pages", &limit(vec![bad])).unwrap_err();
        assert!(error.contains(says), "{error}");
    }
    let twice = limit(vec![
        exception("/agents", 12, "Room."),
        exception("/agents", 13, "More."),
    ]);
    assert!(
        check("pages", &twice)
            .unwrap_err()
            .contains("excepted twice")
    );
}

#[test]
fn a_page_without_an_exception_holds_the_rules_number() {
    let limits = link_docs_check::Limits::read();
    assert_eq!(limits.at("pages", "/no/such/page"), limits.of("pages"));
    for exception in &limits.limit["pages"].except {
        assert_eq!(limits.at("pages", &exception.address), exception.at_most);
        assert!(
            limits
                .table()
                .contains(&format!("at `{}`", exception.address))
        );
    }
}
