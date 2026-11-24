//! The docs' limits, each stated once in `limits.toml` beside this
//! crate, and each workspace's files that were over theirs when the
//! limit came in.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

/// `limits.toml`: each limit by its rule's id, and the generated files
/// no line limit holds for.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Limits {
    pub generated: Vec<String>,
    pub spec_pages: GeneratedPages,
    pub reference_pages: GeneratedPages,
    pub limit: BTreeMap<String, Limit>,
    pub survey: Survey,
    pub rust_names: RustNames,
}

/// Where a page may name Rust source in every workspace (`rust-names`),
/// beside the kinds `kinds.toml` marks `names_rust` and the folders each
/// workspace's `rust-pages.toml` lists.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RustNames {
    /// The folders, at any depth below a member's `docs/`, whose pages
    /// are developer and library references: `more`.
    pub within_docs: Vec<String>,
    /// The manifests that make a folder named `docs` a package, whose
    /// README is no page: `Cargo.toml`.
    pub manifests: Vec<String>,
}

/// Pages `link-actions-surfaces` writes from the declarations, spec pages
/// or command references, and the rules their layout keeps them from.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedPages {
    /// The folder, from a module's own, its pages are written in.
    pub folder: String,
    /// The rules, by id, that don't hold them.
    pub exempt: Vec<String>,
}

/// What `lnk-docs-survey` reports beyond the rules, and from what size: no
/// rule holds these, they're review leads.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Survey {
    /// Characters a paragraph has before it's compared with others.
    pub near_length: usize,
    /// Words in each run two paragraphs are compared by.
    pub near_run: usize,
    /// The share of their runs two paragraphs hold in common to be near
    /// copies.
    pub near_overlap: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limit {
    pub at_most: f64,
    /// The power a ratio's curve raises its measure to, where the limit
    /// is a ratio: `docs-ratio`'s Rust lines to the docs lines expected.
    #[serde(default)]
    pub exponent: Option<f64>,
    pub of: String,
    pub why: String,
    /// The pages held to a number of their own instead, by address, each
    /// saying why: only the rules in [`EXCEPTED`] read them.
    #[serde(default)]
    pub except: Vec<Exception>,
}

/// The rules whose limit a page's address may raise for itself.
pub const EXCEPTED: [&str; 1] = ["pages"];

/// A page a limit holds to a higher number of its own.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    /// The page's address on its website: `/agents/tools`.
    pub address: String,
    pub at_most: usize,
    pub why: String,
}

impl Limit {
    /// The number as the docs write it: `1,000`.
    pub fn number(&self) -> String {
        if self.at_most.fract() != 0.0 {
            return self.at_most.to_string();
        }
        let digits = format!("{:.0}", self.at_most);
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    }
}

impl Limits {
    pub fn read() -> Limits {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("limits.toml");
        let text = std::fs::read_to_string(&path).expect("limits.toml");
        let limits: Limits =
            toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for (id, limit) in &limits.limit {
            assert!(
                limit.at_most.is_finite() && limit.at_most >= 0.0,
                "{id}: a limit is finite and nonnegative"
            );
            assert!(
                id == "docs-ratio" || limit.at_most.fract() == 0.0,
                "{id}: a count limit is a whole number"
            );
            if let Err(error) = check_exceptions(id, limit) {
                panic!("{}: {error}", path.display());
            }
        }
        limits
    }

    /// A rule's limit.
    pub fn of(&self, id: &str) -> usize {
        self.limit
            .get(id)
            .unwrap_or_else(|| panic!("limits.toml has no {id}"))
            .at_most as usize
    }

    /// A rule's limit for the page at `address`: its exception's number
    /// where it has one, else the rule's.
    pub fn at(&self, id: &str, address: &str) -> usize {
        self.limit
            .get(id)
            .and_then(|limit| limit.except.iter().find(|e| e.address == address))
            .map_or_else(|| self.of(id), |exception| exception.at_most)
    }

    /// The table the writing guide shows: each limit, its number and why,
    /// and beneath it each page it holds to a number of its own.
    pub fn table(&self) -> String {
        let mut table = String::from("| Limit | At most | Why |\n|---|---|---|\n");
        for limit in self.limit.values() {
            table += &format!("| {} | {} | {} |\n", limit.of, limit.number(), limit.why);
            for exception in &limit.except {
                table += &format!(
                    "| {}, at `{}` | {} | {} |\n",
                    limit.of, exception.address, exception.at_most, exception.why
                );
            }
        }
        table
    }
}

/// A limit's exceptions are read by its rule, each names one page's
/// address once, raises the limit and says why.
pub fn check_exceptions(id: &str, limit: &Limit) -> Result<(), String> {
    if !limit.except.is_empty() && !EXCEPTED.contains(&id) {
        return Err(format!("{id}: no rule reads its exceptions"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for exception in &limit.except {
        let address = &exception.address;
        if !address.starts_with('/') || (address != "/" && address.ends_with('/')) {
            return Err(format!(
                "{id}: {address}: an exception names a page's address"
            ));
        }
        if !seen.insert(address) {
            return Err(format!("{id}: {address} is excepted twice"));
        }
        if exception.at_most as f64 <= limit.at_most {
            return Err(format!("{id}: {address}: an exception raises the limit"));
        }
        if exception.why.trim().is_empty() {
            return Err(format!("{id}: {address}: an exception says why"));
        }
    }
    Ok(())
}

/// A workspace's `large-files.toml`: each file over its line limit when
/// the limit came in, by its path from the workspace, with its lines
/// then. It may shrink, never grow, and no file joins it.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Large {
    pub lines: BTreeMap<String, usize>,
    /// The pages written before the shape rules, by their path from the
    /// workspace: none joins, and one leaves once it keeps them.
    #[serde(default)]
    pub unshaped: std::collections::BTreeSet<String>,
    /// The packages without a page of their own when every package came
    /// to need one.
    #[serde(default)]
    pub undocumented: std::collections::BTreeSet<String>,
    /// The sections without a hook when every section came to need one.
    #[serde(default)]
    pub unhooked: std::collections::BTreeSet<String>,
    /// What each structure rule held when it came in, by the rule's id:
    /// the pages or packages it would fail. None joins, and each leaves
    /// once it keeps the rule.
    #[serde(default)]
    pub held: BTreeMap<String, std::collections::BTreeSet<String>>,
    /// The folders of data a program reads or writes whole, by their path
    /// from the workspace: records it keeps, or a folder it packs whole.
    /// They're not a map, so neither they nor the folders in them need a
    /// README. Markdown records are raw inputs, not current navigation
    /// or sources of documentation links; raw file checks still hold.
    #[serde(default)]
    pub data: std::collections::BTreeSet<String>,
    /// The folders whose Markdown is IAM's pages, by their path from the
    /// workspace: `iam-pages.toml` beside this list, none without one.
    /// The retired IAM words are refused there ([`crate::words`]).
    #[serde(skip)]
    pub iam: std::collections::BTreeSet<String>,
    /// The folders whose pages may name Rust source, by their path from
    /// the workspace: `rust-pages.toml` beside this list, none without one
    /// ([`crate::rust_names`]).
    #[serde(skip)]
    pub rust: std::collections::BTreeSet<String>,
}

/// A workspace's `iam-pages.toml` or `rust-pages.toml`, beside its
/// `large-files.toml`: the folders a rule reads, by their path from the
/// workspace.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Folders {
    folders: std::collections::BTreeSet<String>,
}

impl Large {
    /// Whether `rule` held `item` when it came in.
    pub fn holds(&self, rule: &str, item: &str) -> bool {
        self.held
            .get(rule)
            .is_some_and(|items| items.contains(item))
    }
}

impl Large {
    pub fn read(path: &Path) -> Large {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let mut large: Large =
            toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        large.iam = folders(&path.with_file_name("iam-pages.toml"));
        large.rust = folders(&path.with_file_name("rust-pages.toml"));
        large
    }
}

/// The folders a workspace's list beside its `large-files.toml` names:
/// none when it has no such list.
fn folders(path: &Path) -> std::collections::BTreeSet<String> {
    std::fs::read_to_string(path).map_or_else(
        |_| std::collections::BTreeSet::new(),
        |text| {
            toml::from_str::<Folders>(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                .folders
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_is_written_as_the_docs_write_it() {
        let limit = |at_most| Limit {
            at_most,
            exponent: None,
            of: String::new(),
            why: String::new(),
            except: Vec::new(),
        };
        assert_eq!(limit(3.5).number(), "3.5");
        assert_eq!(limit(10.0).number(), "10");
        assert_eq!(limit(500.0).number(), "500");
        assert_eq!(limit(1000.0).number(), "1,000");
        assert_eq!(limit(1234567.0).number(), "1,234,567");
    }
}
