//! Paths as people name them in a folder: relative, `/`-separated, and
//! globs over them.

/// Whether `path`, relative and `/`-separated, is `pattern` or inside it. A pattern
/// with `*` or `?` is a glob: `*` and `?` never match `/`, `**` does.
pub fn matches(pattern: &str, path: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    if !pattern.contains(['*', '?']) {
        return path == pattern
            || path
                .strip_prefix(pattern)
                .is_some_and(|rest| rest.starts_with('/'));
    }
    glob(pattern.as_bytes(), path.as_bytes())
}

fn glob(pattern: &[u8], path: &[u8]) -> bool {
    match pattern {
        [] => path.is_empty(),
        [b'*', b'*', b'/', rest @ ..] => {
            // `**/` also matches no folders at all.
            glob(rest, path)
                || (0..path.len())
                    .filter(|&i| path[i] == b'/')
                    .any(|i| glob(rest, &path[i + 1..]))
        }
        [b'*', b'*', rest @ ..] => (0..=path.len()).any(|i| glob(rest, &path[i..])),
        [b'*', rest @ ..] => (0..=path.len())
            .take_while(|&i| i == 0 || path[i - 1] != b'/')
            .any(|i| glob(rest, &path[i..])),
        [b'?', rest @ ..] => path.first().is_some_and(|&c| c != b'/') && glob(rest, &path[1..]),
        [c, rest @ ..] => path.first() == Some(c) && glob(rest, &path[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(matches("photos", "photos/2019/a.jpg"));
        assert!(matches("photos/2019/a.jpg", "photos/2019/a.jpg"));
        assert!(!matches("photo", "photos/a.jpg"));
        assert!(matches("", "anything"));
        assert!(matches("photos/2019/**/*.jpg", "photos/2019/a.jpg"));
        assert!(matches("photos/2019/**/*.jpg", "photos/2019/x/y/a.jpg"));
        assert!(!matches("photos/2019/**/*.jpg", "photos/2019/a.png"));
        assert!(matches("photos/*/a.jpg", "photos/2019/a.jpg"));
        assert!(!matches("photos/*.jpg", "photos/2019/a.jpg"));
        assert!(matches("photos/**", "photos/2019/a.jpg"));
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "a/c"));
    }
}
