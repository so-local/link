//! Click depth: how many links a reader follows from the workspace's
//! root README to each document, reported by `lnk-docs-clicks` and held
//! by two ceilings in `limits.toml` that may only go down.

use std::path::PathBuf;

use crate::navigation::Navigation;
use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[
    Rule {
        id: "clicks",
        scope: Scope::Site,
        fix: "a page is at most this many links from the root README: link it where a reader \
              meets what it explains, never for the link's own sake; the limit may only go down",
        check: Check::Whole(deepest),
    },
    Rule {
        id: "average-clicks",
        scope: Scope::Site,
        fix: "the pages are on average at most this many hundredths of a link from the root \
              README: link a page where a reader meets what it explains; the limit may only go down",
        check: Check::Whole(average),
    },
];

/// A workspace's click depth over every document but its root README.
#[derive(Debug, PartialEq)]
pub struct Clicks {
    /// The documents measured, each reached from the root README.
    pub pages: usize,
    /// Their average clicks, in hundredths, rounded up.
    pub average: usize,
    /// The clicks of the middle one, the lower of two in the middle.
    pub median: usize,
    /// The most clicks any takes.
    pub deepest: usize,
    /// The documents that many clicks away.
    pub deepest_pages: Vec<PathBuf>,
}

/// Measures `tree`'s documents as the reading tree reaches them, for
/// `rule`: a page it doesn't hold left out.
pub fn measure(tree: &Tree, rule: &str) -> Clicks {
    let root = tree.root.join("README.md");
    let reached: Vec<(PathBuf, usize)> = Navigation::new(tree)
        .clicks()
        .into_iter()
        .filter(|(path, _)| *path != root)
        // A generated page is looked up from its index, never read on the
        // way to another: the authored pages make the reading tree
        // (`limits.toml`).
        .filter(|(path, _)| {
            !tree
                .text(path)
                .is_some_and(|text| tree.generated_exempt(rule, path, text))
        })
        .collect();
    of(reached)
}

fn of(reached: Vec<(PathBuf, usize)>) -> Clicks {
    let mut depths: Vec<usize> = reached.iter().map(|(_, clicks)| *clicks).collect();
    depths.sort_unstable();
    let pages = depths.len();
    let total: usize = depths.iter().sum();
    let deepest = depths.last().copied().unwrap_or_default();
    Clicks {
        pages,
        average: (total * 100).div_ceil(pages.max(1)),
        median: depths
            .get(pages.saturating_sub(1) / 2)
            .copied()
            .unwrap_or_default(),
        deepest,
        deepest_pages: reached
            .into_iter()
            .filter(|(_, clicks)| *clicks == deepest)
            .map(|(path, _)| path)
            .collect(),
    }
}

/// The report `lnk-docs-clicks` prints.
pub fn report(tree: &Tree) -> String {
    let clicks = measure(tree, "clicks");
    let mut text = format!(
        "{}: {} pages, {}.{:02} clicks on average, median {}, deepest {}\n",
        tree.root.display(),
        clicks.pages,
        clicks.average / 100,
        clicks.average % 100,
        clicks.median,
        clicks.deepest,
    );
    for page in &clicks.deepest_pages {
        text += &format!("  {} clicks: {}\n", clicks.deepest, tree.shown(page));
    }
    text
}

fn deepest(tree: &Tree) -> Vec<Finding> {
    let most = tree.limits.of("clicks");
    let clicks = measure(tree, "clicks");
    clicks
        .deepest_pages
        .iter()
        .filter(|_| clicks.deepest > most)
        .map(|page| {
            fail(format!(
                "{}: {} clicks from README.md, past {most}",
                tree.shown(page),
                clicks.deepest
            ))
        })
        .collect()
}

fn average(tree: &Tree) -> Vec<Finding> {
    let most = tree.limits.of("average-clicks");
    let clicks = measure(tree, "average-clicks");
    if clicks.average <= most {
        return Vec::new();
    }
    vec![fail(format!(
        "README.md: its pages are {} hundredths of a click away on average, past {most}",
        clicks.average
    ))]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_average_rounded_up_with_the_lower_middle_and_the_deepest_named() {
        let page = |name: &str, clicks| (PathBuf::from(name), clicks);
        let clicks = of(vec![page("a", 1), page("b", 2), page("c", 4), page("d", 4)]);
        assert_eq!(
            clicks,
            Clicks {
                pages: 4,
                average: 275,
                median: 2,
                deepest: 4,
                deepest_pages: vec![PathBuf::from("c"), PathBuf::from("d")],
            }
        );
        assert_eq!(
            of(vec![page("a", 1), page("b", 1), page("c", 2)]).average,
            134
        );
        assert_eq!(of(Vec::new()).pages, 0);
    }
}
