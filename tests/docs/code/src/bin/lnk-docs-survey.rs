//! Survey a workspace's docs: how pages are reached, how deep they sit,
//! how much each package says for its code, which commands its pages
//! show, and what each structure rule still holds. It reads what the
//! rules read, with the same measures, and prints a Markdown report.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use link_docs_check::Tree;
use link_docs_check::structure::{
    Links, address_depth, addresses, links_by_block, measures, near_copies, page_depth, repeated,
    sidebar_strays,
};
use link_docs_check_code::coverage::{authored, declared};

#[path = "../survey.rs"]
mod survey;

#[derive(clap::Parser)]
#[command(about = "Survey workspace documentation")]
struct Cli {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long)]
    large: Option<PathBuf>,
    #[arg(long)]
    reviews: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run() {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("lnk-docs-survey: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String, String> {
    let Cli {
        root,
        large,
        reviews,
    } = link_spec::entry::plain();
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let large = large.unwrap_or_else(|| root.join("tests/docs/large-files.toml"));
    if !large.exists() {
        return Err(format!(
            "{}: no such file; name the lists with --large",
            large.display()
        ));
    }
    let tree = Tree::read(&root, false, &large);
    let mut out = String::new();
    held(&tree, &mut out);
    reach(&tree, &mut out);
    depth(&tree, &mut out);
    leads(&tree, &mut out);
    packages(&tree, &mut out);
    commands(&tree, &mut out);
    survey::owners(&tree, &mut out)?;
    survey::outline(&tree, &mut out);
    if let Some(path) = reviews {
        let record = link_docs_check_code::reviews::Reviews::read(&path)?;
        reviewed(&tree, &record, &mut out);
    }
    Ok(out)
}

/// Each owner with docs, by its path from the repository's top, its last review
/// and how many commits have touched it since; the owners never
/// reviewed first.
#[expect(
    clippy::disallowed_methods,
    reason = "git names the repository's top and counts the commits since each review"
)]
fn reviewed(tree: &Tree, record: &link_docs_check_code::reviews::Reviews, out: &mut String) {
    let mut owners: Vec<PathBuf> = measures(tree)
        .into_iter()
        .filter(|m| m.pages > 1 || tree.dirs.contains(&m.package.join("docs")))
        .map(|m| m.package)
        .collect();
    if tree.dirs.contains(&tree.root.join("docs")) {
        owners.push(tree.root.clone());
    }
    // Owners are named from the repository's top, so one record holds
    // every workspace of a checkout.
    let top = std::process::Command::new("git")
        .arg("-C")
        .arg(&tree.root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or_else(|| tree.root.clone());
    let key = |owner: &Path| {
        let key = owner
            .strip_prefix(&top)
            .unwrap_or(owner)
            .display()
            .to_string();
        if key.is_empty() { ".".to_string() } else { key }
    };
    let since = |commit: &str, owner: &Path| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&top)
            .args(["rev-list", "--count", &format!("{commit}..HEAD"), "--"])
            .arg(owner)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| "?".into())
    };
    let never: Vec<String> = owners
        .iter()
        .map(|o| key(o))
        .filter(|k| !record.reviewed.contains_key(k))
        .collect();
    let _ = writeln!(
        out,
        "## Reviews\n\n{} of {} owners with docs reviewed.\n\n| Owner | Reviewed | Commit | Outcome | Commits since | Left |\n|---|---|---|---|---|---|",
        owners.len() - never.len(),
        owners.len()
    );
    for owner in &never {
        let _ = writeln!(out, "| `{owner}` | never | | | | |");
    }
    for owner in &owners {
        let Some(review) = record.reviewed.get(&key(owner)) else {
            continue;
        };
        let _ = writeln!(
            out,
            "| `{}` | {} | `{}` | {:?} | {} | {} |",
            key(owner),
            review.date,
            review.commit,
            review.outcome,
            since(&review.commit, owner),
            review.left.as_deref().unwrap_or("")
        );
    }
    let _ = writeln!(out);
}

/// A count per value, as `value: count` pairs in order.
fn histogram(values: impl IntoIterator<Item = usize>) -> String {
    let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
    for v in values {
        *counts.entry(v).or_default() += 1;
    }
    counts
        .iter()
        .map(|(v, n)| format!("{v}: {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn held(tree: &Tree, out: &mut String) {
    let _ = writeln!(out, "# Docs Survey\n\n## Held\n");
    let _ = writeln!(out, "| Rule | Held |\n|---|---|");
    for (rule, items) in &tree.large.held {
        let _ = writeln!(out, "| `{rule}` | {} |", items.len());
    }
    let _ = writeln!(out);
}

/// Hops from the home page to each page, following `follow`'s links.
fn hops(
    home: &Path,
    links: &BTreeMap<PathBuf, Links>,
    follow: fn(&Links) -> Vec<&PathBuf>,
) -> BTreeMap<PathBuf, usize> {
    let mut seen = BTreeMap::from([(home.to_path_buf(), 0)]);
    let mut queue = VecDeque::from([home.to_path_buf()]);
    while let Some(page) = queue.pop_front() {
        let at = seen[&page];
        for next in links.get(&page).map(follow).unwrap_or_default() {
            if !seen.contains_key(next) {
                seen.insert(next.clone(), at + 1);
                queue.push_back(next.clone());
            }
        }
    }
    seen
}

fn reach(tree: &Tree, out: &mut String) {
    let addresses = addresses(tree);
    let Some(home) = addresses
        .iter()
        .find(|(_, a)| a.as_str() == "/")
        .map(|(f, _)| f.clone())
    else {
        let _ = writeln!(out, "## Reach\n\nNo website: no pages to reach.\n");
        return;
    };
    let pages: BTreeSet<&PathBuf> = addresses.keys().collect();
    let links: BTreeMap<PathBuf, Links> = addresses
        .keys()
        .map(|f| {
            let text = tree.text(f).unwrap_or_default();
            (f.clone(), links_by_block(f, text, &pages))
        })
        .collect();
    let count = |pick: fn(&Links) -> usize| links.values().map(pick).sum::<usize>();
    let prose = hops(&home, &links, |l| l.paragraphs.iter().collect());
    let all = hops(&home, &links, |l| {
        l.paragraphs
            .iter()
            .chain(&l.lists)
            .chain(&l.tables)
            .collect()
    });
    let unlinked = addresses
        .keys()
        .filter(|f| **f != home && !links.values().any(|l| l.paragraphs.contains(*f)))
        .count();
    let total = addresses.len() - 1;
    let _ = writeln!(
        out,
        "## Reach\n\nLinks between pages: {} in paragraphs, {} in lists, {} in tables.\n",
        count(|l| l.paragraphs.len()),
        count(|l| l.lists.len()),
        count(|l| l.tables.len())
    );
    let _ = writeln!(
        out,
        "| Following | Pages reached | Hops (hops: pages) |\n|---|---|---|\n| Paragraphs | {} of {total} | {} |\n| Every link | {} of {total} | {} |\n",
        prose.len() - 1,
        histogram(prose.values().copied().filter(|h| *h > 0)),
        all.len() - 1,
        histogram(all.values().copied().filter(|h| *h > 0)),
    );
    let _ = writeln!(
        out,
        "Pages no paragraph links to: {unlinked} of {total}. Paragraphs on two pages or more: {}.\n",
        repeated(tree).len()
    );
}

fn depth(tree: &Tree, out: &mut String) {
    let addresses = addresses(tree);
    if addresses.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "## Depth\n\n| Measure | Levels (levels: pages) |\n|---|---|\n| Address | {} |\n| Address and headings | {} |\n",
        histogram(addresses.values().map(|a| address_depth(a))),
        histogram(
            addresses
                .iter()
                .map(|(f, a)| page_depth(a, tree.text(f).unwrap_or_default()))
        ),
    );
}

/// What no rule holds but a review should read: near copies, and pages
/// the sidebar nests away from their address.
fn leads(tree: &Tree, out: &mut String) {
    let near = near_copies(tree);
    let strays = sidebar_strays(tree);
    let _ = writeln!(
        out,
        "## Review Leads\n\nParagraph pairs on two pages that are near copies: {}. Pages the sidebar nests under a page their address isn't below: {}.\n",
        near.len(),
        strays.len()
    );
    if !near.is_empty() {
        let _ = writeln!(out, "| Overlap | Page | Page |\n|---|---|---|");
        for (overlap, a, b) in &near {
            let _ = writeln!(
                out,
                "| {overlap:.2} | `{}` | `{}` |",
                tree.shown(a),
                tree.shown(b)
            );
        }
        let _ = writeln!(out);
    }
    if !strays.is_empty() {
        let _ = writeln!(out, "| Page | Sidebar puts it under |\n|---|---|");
        for (page, under) in &strays {
            let _ = writeln!(out, "| `{}` | `{}` |", tree.shown(page), tree.shown(under));
        }
        let _ = writeln!(out);
    }
}

fn packages(tree: &Tree, out: &mut String) {
    let limit = &tree.limits.limit["docs-ratio"];
    let exponent = limit.exponent.unwrap_or(1.0);
    let mut rows = measures(tree);
    rows.sort_by(|a, b| {
        (a.rust == 0)
            .cmp(&(b.rust == 0))
            .then_with(|| b.off_by(exponent).total_cmp(&a.off_by(exponent)))
    });
    let rust: usize = rows.iter().map(|m| m.rust).sum();
    let docs: usize = rows.iter().map(|m| m.docs).sum();
    let _ = writeln!(
        out,
        "## Packages\n\n{} Cargo and npm owners: {docs} authored docs lines for {rust} ratio-input Rust lines. Expected docs are Rust lines to the {exponent}; `docs-ratio` allows {} times off either way. Source Rust keeps the established ratio; test-only packages use their maintained test implementation instead of an empty library stub. Production, test, and declared generated counts remain separate.\n",
        rows.len(),
        limit.at_most
    );
    let _ = writeln!(
        out,
        "| Package | Ratio Rust | Production | Tests | Generated | Docs | Words | Terminal branches | Supporting | Pages | Expected | Off by |\n|---|---|---|---|---|---|---|---|---|---|---|---|"
    );
    for m in &rows {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            tree.shown(&m.package),
            m.rust,
            m.production,
            m.tests,
            m.generated,
            m.docs,
            m.words,
            m.terminal_examples,
            m.supporting_pages,
            m.pages,
            if m.rust == 0 {
                "N/A".into()
            } else {
                format!("{:.0}", m.expected(exponent))
            },
            if m.rust == 0 {
                "N/A".into()
            } else {
                format!("{:.1}", m.off_by(exponent))
            }
        );
        for problem in &m.problems {
            let _ = writeln!(out, "\nUnresolved measurement: {problem}\n");
        }
        for problem in &m.example_problems {
            let _ = writeln!(out, "\nUnresolved terminal example: {problem}\n");
        }
    }
    let _ = writeln!(out);
}

fn commands(tree: &Tree, out: &mut String) {
    let declared = declared(tree);
    let mut owners: BTreeMap<String, [usize; 4]> = BTreeMap::new();
    let mut texts: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for command in declared.iter().filter(|c| c.path != "root") {
        let owner = command.documentation_owner(tree);
        let row = owners.entry(tree.shown(&owner)).or_default();
        if !command.offered() {
            row[if command.action.hidden() { 2 } else { 3 }] += 1;
            continue;
        }
        row[0] += 1;
        let pages = texts.entry(owner.clone()).or_insert_with(|| {
            tree.files
                .iter()
                .filter(|(p, _)| {
                    p.starts_with(&owner)
                        && p.extension().is_some_and(|e| e == "md")
                        && p.components().any(|c| c.as_os_str() == "docs")
                })
                .map(|(_, t)| authored(t))
                .collect()
        });
        if pages.iter().any(|t| command.shown_in(t)) {
            row[1] += 1;
        }
    }
    let _ = writeln!(
        out,
        "## Commands\n\n| Owner | Offered | Shown on its pages | Hidden | Other surfaces |\n|---|---|---|---|---|"
    );
    for (owner, [offered, shown, hidden, other]) in &owners {
        let _ = writeln!(
            out,
            "| `{owner}` | {offered} | {shown} | {hidden} | {other} |"
        );
    }
    let _ = writeln!(out);
}

#[cfg(test)]
link_test_kit::program!(system);
