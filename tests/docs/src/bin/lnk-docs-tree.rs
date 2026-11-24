//! Update persisted site outlines and README indexes, or check their freshness.

use std::path::PathBuf;
use std::process::ExitCode;

use link_docs_check::Tree;
use link_docs_check::navigation::Navigation;

/// One of `--write` or `--check`, never neither: a run says which it is.
#[derive(clap::Parser)]
#[command(about = "Check or update site outlines and README indexes")]
#[command(group(clap::ArgGroup::new("mode").required(true).args(["write", "check"])))]
struct Cli {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Write every stale outline, index and generated reference.
    #[arg(long)]
    write: bool,
    /// Refuse a stale or unreachable page, writing nothing.
    #[arg(long)]
    check: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("docs-tree: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let Cli { root, write, check } = link_spec::entry::plain();
    debug_assert!(write != check, "the mode group takes exactly one");
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    // The references first, so the indexes below list a page they add.
    let references = link_docs_check::references::files(&root).map_err(|e| format!("{e:#}"))?;
    if write {
        let changed = references.write(&root).map_err(|e| format!("{e:#}"))?;
        println!("{changed} generated references updated");
    }
    // The indexed workspace's own list, which says what is data there;
    // this check's own where the root holds none.
    let large = link_docs_check::large_files_of(&root)?
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("large-files.toml"));
    let tree = Tree::read(&root, false, &large);
    let navigation = Navigation::new(&tree);
    let updates = navigation.updates()?;
    if write {
        for update in &updates {
            std::fs::write(&update.path, &update.text)
                .map_err(|e| format!("{}: {e}", update.path.display()))?;
        }
        println!(
            "{} documents; {} navigation files updated",
            navigation.document_count(),
            updates.len()
        );
        return Ok(());
    }
    for update in &updates {
        eprintln!("{}: missing or stale navigation", tree.shown(&update.path));
    }
    let problems = references.problems(&root).map_err(|e| format!("{e:#}"))?;
    for problem in &problems {
        eprintln!("{problem}");
    }
    let unreachable = navigation.unreachable();
    for path in &unreachable {
        eprintln!("{}: no reading path from README.md", tree.shown(path));
    }
    println!(
        "{} documents; {} stale indexes; {} unreachable documents",
        navigation.document_count(),
        updates.len(),
        unreachable.len()
    );
    if updates.is_empty() && unreachable.is_empty() && problems.is_empty() {
        Ok(())
    } else {
        Err("run again with --write to regenerate navigation and references".into())
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
