//! Where a search or a list carries on ([`Page`]): its answer's `next`,
//! given back as the next call's `page`. A list's page names the last
//! document it gave by when it changed and its path, so the next is read
//! from the index's order there, never by passing over what came before;
//! a search's names how far into what it ranked the last hit was.

use anyhow::Result;

/// Where a call carries on, as its `page` names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    /// A search's: after this many of what it ranks.
    Search { at: usize },
    /// A list's: after the document that changed at `changed` (the
    /// index's nanoseconds), at `path`.
    List { changed: i64, path: String },
}

impl Page {
    /// The page `token` names, as a call of `kind` takes it (`search` or
    /// `list`): an error for a page of another call, or none.
    pub fn parse(token: &str, kind: &str) -> Result<Page> {
        let not = || anyhow::anyhow!("{token:?} isn't a page of a {kind}: give the 'next' it gave");
        let page = match token.split_at_checked(1) {
            Some(("s", at)) => Page::Search {
                at: at.parse().map_err(|_| not())?,
            },
            Some(("l", rest)) => {
                let (changed, path) = rest.split_once('.').ok_or_else(not)?;
                Page::List {
                    changed: changed.parse().map_err(|_| not())?,
                    path: Some(path)
                        .filter(|path| !path.is_empty())
                        .ok_or_else(not)?
                        .to_string(),
                }
            }
            _ => return Err(not()),
        };
        match (&page, kind) {
            (Page::Search { .. }, "search") | (Page::List { .. }, "list") => Ok(page),
            _ => Err(not()),
        }
    }

    /// The page as a call's `next` gives it.
    pub fn token(&self) -> String {
        match self {
            Page::Search { at } => format!("s{at}"),
            Page::List { changed, path } => format!("l{changed}.{path}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_reads_back_as_itself_and_only_for_its_call() {
        for page in [
            Page::Search { at: 12 },
            Page::List {
                changed: 1_790_690_587_000_000_000,
                path: "memory/taxes/2026 ünd.md".into(),
            },
        ] {
            let kind = match page {
                Page::Search { .. } => "search",
                Page::List { .. } => "list",
            };
            assert_eq!(Page::parse(&page.token(), kind).unwrap(), page);
        }
        assert!(Page::parse("s12", "list").is_err());
        for bad in ["", "s", "sx", "l12", "l12.", "lx.memory/a.md", "x1"] {
            assert!(Page::parse(bad, "list").is_err(), "{bad}");
            assert!(Page::parse(bad, "search").is_err(), "{bad}");
        }
    }
}
