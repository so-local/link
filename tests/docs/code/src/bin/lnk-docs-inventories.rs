//! Regenerate or check inventories from their existing declarations and
//! pages, the generated headers of the file kinds that carry one, and the
//! links from a page's first mention of a command to its owner's section.
//! `--root <workspace>` writes or checks another workspace's headers alone;
//! `--large <large-files.toml>` names its held lists, the workspace's
//! `tests/docs/large-files.toml` by default.

use std::path::{Path, PathBuf};

/// One of `--write` or `--check`, never neither: a run says which it is.
#[derive(clap::Parser)]
#[command(group(clap::ArgGroup::new("mode").required(true).args(["write", "check"])))]
struct Cli {
    /// Write every stale inventory, header and owner link.
    #[arg(long)]
    write: bool,
    /// Refuse a stale one, writing nothing.
    #[arg(long)]
    check: bool,
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    large: Option<PathBuf>,
}

fn main() -> Result<(), String> {
    let Cli {
        root: other,
        large,
        write,
        check,
    } = link_spec::entry::plain();
    debug_assert!(write != check, "the mode group takes exactly one");
    let public = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let root = other.clone().unwrap_or(public);
    let large = large.unwrap_or_else(|| root.join("tests/docs/large-files.toml"));
    if !large.is_file() {
        return Err(format!(
            "{}: no such file; name the held lists with --large",
            large.display()
        ));
    }
    let tree = link_docs_check::Tree::read(&root, false, &large);
    let mut updates = Vec::new();
    if other.is_none() {
        updates.extend(link_docs_check_code::inventories::updates(&tree)?);
        updates.extend(link_docs_check_code::references::updates(&tree)?);
        updates.extend(link_docs_check_code::owner_links::updates(&tree));
    }
    updates.extend(link_docs_check_code::files::updates(&tree)?);
    if write {
        for update in &updates {
            std::fs::write(&update.path, &update.text).map_err(|e| e.to_string())?;
        }
        println!("{} inventories updated", updates.len());
    } else if !updates.is_empty() {
        return Err("stale inventories; run lnk-docs-inventories --write".into());
    }
    Ok(())
}

#[cfg(test)]
link_test_kit::program!(unit);
