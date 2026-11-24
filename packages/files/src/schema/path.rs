//! A step's path: the keys from the document's top to a field, joined by
//! dots. A key a bare TOML key can't spell, as one holding a dot, is
//! quoted, `locations."lnk.migration-journal"`, and only such a key, so each field
//! has one path.

/// Whether `key` is spelled bare in a path: letters, digits, `-` and `_`,
/// as a bare TOML key is.
fn bare(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `key` as a path spells it: quoted when it can't be bare.
pub fn key(key: &str) -> String {
    match bare(key) {
        true => key.to_string(),
        false => format!("\"{key}\""),
    }
}

/// The path of `keys`, from the top.
pub fn join<'a>(keys: impl IntoIterator<Item = &'a str>) -> String {
    keys.into_iter().map(key).collect::<Vec<_>>().join(".")
}

/// Whether `path` is one: keys, each bare or quoted where it must be,
/// none empty, none holding a quote and none quoted holding a `\`, which
/// a TOML quoted key reads as an escape a path never spells.
pub fn check(path: &str) -> bool {
    checked(path, false)
}

/// Whether `path` is a pattern of paths: [`check`], where a bare `*`
/// stands for any one key.
pub fn check_pattern(path: &str) -> bool {
    checked(path, true)
}

fn checked(path: &str, any: bool) -> bool {
    let mut rest = path;
    loop {
        let (key, quoted, after) = match rest.strip_prefix('"') {
            Some(inner) => match inner.find('"') {
                Some(end) => (&inner[..end], true, &inner[end + 1..]),
                None => return false,
            },
            None => match rest.find('.') {
                Some(at) => (&rest[..at], false, &rest[at..]),
                None => (rest, false, ""),
            },
        };
        let star = any && !quoted && key == "*";
        if key.is_empty()
            || key.contains('"')
            || (quoted && key.contains('\\'))
            || (!star && bare(key) == quoted)
        {
            return false;
        }
        match after {
            "" => return true,
            _ => match after.strip_prefix('.') {
                Some(next) => rest = next,
                None => return false,
            },
        }
    }
}

/// The first key of `path`, without its quotes, and the path after it.
pub fn split_first(path: &str) -> Option<(&str, Option<&str>)> {
    let (head, after) = match path.strip_prefix('"') {
        Some(inner) => {
            let end = inner.find('"')?;
            (&inner[..end], &inner[end + 1..])
        }
        None => match path.find('.') {
            Some(at) => (&path[..at], &path[at..]),
            None => (path, ""),
        },
    };
    match after {
        "" => Some((head, None)),
        _ => Some((head, Some(after.strip_prefix('.')?))),
    }
}

/// The keys of `path`, without their quotes.
pub fn keys(path: &str) -> Option<Vec<&str>> {
    let mut keys = Vec::new();
    let mut rest = Some(path);
    while let Some(path) = rest {
        let (head, next) = split_first(path)?;
        keys.push(head);
        rest = next;
    }
    Some(keys)
}

/// The path of the table holding `path`'s field, none at the top, and the
/// field's own key, without its quotes.
pub fn split_last(path: &str) -> Option<(Option<&str>, &str)> {
    let mut rest = path;
    loop {
        match split_first(rest)? {
            (head, None) => {
                let parent =
                    (rest.len() < path.len()).then(|| &path[..path.len() - rest.len() - 1]);
                return Some((parent, head));
            }
            (_, Some(next)) => rest = next,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_key_holds_a_dot() {
        let path = r#"locations."lnk.migration-journal".path"#;
        assert!(check(path));
        assert_eq!(
            keys(path).unwrap(),
            ["locations", "lnk.migration-journal", "path"]
        );
        assert_eq!(
            split_first(path),
            Some(("locations", Some(r#""lnk.migration-journal".path"#)))
        );
        assert_eq!(
            split_last(path),
            Some((Some(r#"locations."lnk.migration-journal""#), "path"))
        );
        assert_eq!(
            split_last(r#"locations."lnk.migration-journal""#),
            Some((Some("locations"), "lnk.migration-journal"))
        );
        assert_eq!(split_last("port"), Some((None, "port")));
        assert_eq!(join(["locations", "lnk.migration-journal", "path"]), path);
    }

    /// Each field has one path: a key that can be bare is never quoted,
    /// and a broken quote, an empty key or a dot outside quotes is no key.
    #[test]
    fn one_spelling_each() {
        for path in ["server.port", "a-b.c_d", r#""a.b""#, r#"x."a b".y"#] {
            assert!(check(path), "{path}");
        }
        for path in [
            "",
            "a..b",
            ".a",
            "a.",
            r#"locations."lnk""#,
            r#""a.b"#,
            r#""a"b"#,
            r#"a."""#,
            r#"a"b.c"#,
            "a b",
            r#"a."b\\c""#,
            r#""a\".b""#,
        ] {
            assert!(!check(path), "{path}");
        }
        assert!(!check("*.id") && check_pattern("*.id"));
        assert!(check_pattern(r#"*."lnk.migration-journal""#));
        assert!(!check_pattern("a*.id"));
    }
}
