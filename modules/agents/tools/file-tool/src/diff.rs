//! A text file's change as a unified diff ([`unified`]), as `diff -u`
//! and `git diff` write one, and the change taken back ([`reverse`]):
//! what an agent's file tool shows of each change it makes, and what
//! takes a file back to before it.
//!
//! Lines are compared whole, their ends kept: a file whose last line has
//! no end says so (`\ No newline at end of file`), so a change taken back
//! gives the file byte for byte.

/// The lines of context kept around each change.
pub const CONTEXT: usize = 3;

/// The most edits the comparison looks for before it gives up on a
/// shortest diff and replaces what's between the common head and tail
/// whole: correct, only longer. Keeps a rewrite of a large file quick,
/// and its trace, a reach per edit, small.
const EDITS_MAX: usize = 1024;

/// File `path`'s change from `before` to `after` as a unified diff, with
/// [`CONTEXT`] lines around each change: empty when they're the same.
/// `before` of `None` is a file made (`--- /dev/null`), `after` of `None`
/// one removed (`+++ /dev/null`).
pub fn unified(path: &str, before: Option<&str>, after: Option<&str>) -> String {
    let old = lines(before.unwrap_or_default());
    let new = lines(after.unwrap_or_default());
    if before.is_some() && before == after {
        return String::new();
    }
    let ops = edits(&old, &new);
    let mut out = format!(
        "--- {}\n+++ {}\n",
        before.map_or("/dev/null".to_string(), |_| format!("a/{path}")),
        after.map_or("/dev/null".to_string(), |_| format!("b/{path}")),
    );
    for hunk in hunks(&ops) {
        let (mut old_len, mut new_len) = (0, 0);
        let mut body = String::new();
        for op in &ops[hunk.clone()] {
            let (mark, line) = match *op {
                Op::Same(i, _) => {
                    old_len += 1;
                    new_len += 1;
                    (' ', old[i])
                }
                Op::Gone(i) => {
                    old_len += 1;
                    ('-', old[i])
                }
                Op::Added(j) => {
                    new_len += 1;
                    ('+', new[j])
                }
            };
            body.push(mark);
            body.push_str(line);
            if !line.ends_with('\n') {
                body.push_str("\n\\ No newline at end of file\n");
            }
        }
        let (old_at, new_at) = starts(&ops, hunk.start);
        out.push_str(&format!(
            "@@ -{} +{} @@\n",
            range(old_at, old_len),
            range(new_at, new_len)
        ));
        out.push_str(&body);
    }
    out
}

/// The text a file had before the change `diff` (as [`unified`] wrote it)
/// made, from `now`, the text it has: `None` when `now` isn't what the
/// change left, at the place each part of it says, so the file changed
/// since and is left as it is. A diff of a file made gives an empty text.
pub fn reverse(diff: &str, now: &str) -> Option<String> {
    let current = lines(now);
    let mut out = String::new();
    // The next line of `current` not yet passed.
    let mut at = 0;
    let mut diff_lines = diff.split_inclusive('\n').peekable();
    // Its two headers.
    for header in ["--- ", "+++ "] {
        diff_lines.next().filter(|l| l.starts_with(header))?;
    }
    while let Some(head) = diff_lines.next() {
        let (_, new_at) = hunk_starts(head)?;
        // The hunk's new side starts after its line `new_at`, or at it.
        let start = new_at.saturating_sub(1);
        if start < at || start > current.len() {
            return None;
        }
        for line in &current[at..start] {
            out.push_str(line);
        }
        at = start;
        let mut old_side = String::new();
        let mut new_side = String::new();
        while let Some(line) = diff_lines.next_if(|l| !l.starts_with("@@ ")) {
            let (mark, text) = line.split_at_checked(1)?;
            // A line without its end is followed by the marker saying so.
            let ended = !diff_lines.peek().is_some_and(|next| next.starts_with('\\'));
            let text = match ended {
                true => text,
                false => text.strip_suffix('\n').unwrap_or(text),
            };
            match mark {
                " " => {
                    old_side.push_str(text);
                    new_side.push_str(text);
                }
                "-" => old_side.push_str(text),
                "+" => new_side.push_str(text),
                "\\" => {}
                _ => return None,
            }
        }
        // What it left there is still what's there.
        let count = lines(&new_side).len();
        if at + count > current.len() || current[at..at + count].concat() != new_side {
            return None;
        }
        out.push_str(&old_side);
        at += count;
    }
    for line in &current[at..] {
        out.push_str(line);
    }
    Some(out)
}

/// `text`'s lines, each with its end.
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// A step of the comparison: a line both have (its place in each), one
/// only the old has, or one only the new has.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Same(usize, usize),
    Gone(usize),
    Added(usize),
}

/// The steps from `old` to `new`: their common head and tail, and a
/// shortest edit between (Myers), else, past [`EDITS_MAX`], the middle
/// replaced whole.
fn edits(old: &[&str], new: &[&str]) -> Vec<Op> {
    let head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (a, b) = (&old[head..old.len() - tail], &new[head..new.len() - tail]);
    let mut ops: Vec<Op> = (0..head).map(|i| Op::Same(i, i)).collect();
    match shortest(a, b) {
        Some(middle) => ops.extend(middle.into_iter().map(|op| match op {
            Op::Same(i, j) => Op::Same(head + i, head + j),
            Op::Gone(i) => Op::Gone(head + i),
            Op::Added(j) => Op::Added(head + j),
        })),
        None => {
            ops.extend((0..a.len()).map(|i| Op::Gone(head + i)));
            ops.extend((0..b.len()).map(|j| Op::Added(head + j)));
        }
    }
    let (old_tail, new_tail) = (old.len() - tail, new.len() - tail);
    ops.extend((0..tail).map(|k| Op::Same(old_tail + k, new_tail + k)));
    ops
}

/// A shortest edit from `a` to `b` (Myers' O(ND)), its removals before
/// its additions where they meet; `None` past [`EDITS_MAX`] edits.
fn shortest(a: &[&str], b: &[&str]) -> Option<Vec<Op>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let most = (a.len() + b.len()).min(EDITS_MAX) as isize;
    let offset = most + 1;
    let mut v = vec![0isize; 2 * most as usize + 3];
    // Each round's furthest reach, to walk back from the end.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    'search: for d in 0..=most {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let i = (k + offset) as usize;
            let mut x = match k == -d || (k != d && v[i - 1] < v[i + 1]) {
                true => v[i + 1],
                false => v[i - 1] + 1,
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[i] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'search;
            }
            k += 2;
        }
    }
    let d_end = found?;
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..=d_end).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let i = (k + offset) as usize;
        let prev_k = match k == -d || (k != d && v[i - 1] < v[i + 1]) {
            true => k + 1,
            false => k - 1,
        };
        let prev_x = v[(prev_k + offset) as usize];
        let prev_y = prev_x - prev_k;
        // The snake down to where this round's edit ended.
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            ops.push(Op::Same(x as usize, y as usize));
        }
        if d == 0 {
            break;
        }
        match x == prev_x {
            true => ops.push(Op::Added(prev_y as usize)),
            false => ops.push(Op::Gone(prev_x as usize)),
        }
        (x, y) = (prev_x, prev_y);
    }
    ops.reverse();
    Some(ops)
}

/// The steps of each hunk: the changes, with [`CONTEXT`] lines around
/// each, those whose context meets made one.
fn hunks(ops: &[Op]) -> Vec<std::ops::Range<usize>> {
    let mut hunks: Vec<std::ops::Range<usize>> = Vec::new();
    for (n, op) in ops.iter().enumerate() {
        if matches!(op, Op::Same(..)) {
            continue;
        }
        let start = n.saturating_sub(CONTEXT);
        let end = (n + 1 + CONTEXT).min(ops.len());
        match hunks.last_mut() {
            Some(last) if start <= last.end => last.end = end,
            _ => hunks.push(start..end),
        }
    }
    hunks
}

/// Where the step at `n` is in the old and the new, from 0.
fn starts(ops: &[Op], n: usize) -> (usize, usize) {
    ops[..n].iter().fold((0, 0), |(old, new), op| match op {
        Op::Same(..) => (old + 1, new + 1),
        Op::Gone(_) => (old + 1, new),
        Op::Added(_) => (old, new + 1),
    })
}

/// A hunk's range as its head writes it: its first line, from 1, and how
/// many; an empty one names the line before it.
fn range(at: usize, len: usize) -> String {
    match len {
        0 => format!("{at},0"),
        1 => format!("{}", at + 1),
        len => format!("{},{len}", at + 1),
    }
}

/// A hunk head's (`@@ -1,3 +1,4 @@`) first lines, old and new, from 1 (0
/// for an empty side at the file's start).
fn hunk_starts(head: &str) -> Option<(usize, usize)> {
    let rest = head.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, _) = rest.split_once(" @@")?;
    let first = |range: &str| range.split(',').next()?.parse::<usize>().ok();
    Some((first(old)?, first(new)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(before: &str, after: &str) -> String {
        let diff = unified("work/a.md", Some(before), Some(after));
        assert_eq!(
            reverse(&diff, after).as_deref(),
            Some(before),
            "taking back:\n{diff}"
        );
        diff
    }

    #[test]
    fn a_change_shows_as_git_shows_it() {
        let before = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n";
        let after = "one\ntwo\nthree\nFOUR\nfive\nsix\nseven\neight\nnine\n";
        assert_eq!(
            round_trip(before, after),
            "--- a/work/a.md\n+++ b/work/a.md\n@@ -1,8 +1,9 @@\n one\n two\n three\n-four\n+FOUR\n \
             five\n six\n seven\n eight\n+nine\n"
        );
    }

    #[test]
    fn changes_apart_are_hunks_of_their_own() {
        let before: String = (1..=30).map(|n| format!("{n}\n")).collect();
        let after: String = (1..=30)
            .filter(|n| *n != 27)
            .map(|n| match n {
                3 => "three\n".to_string(),
                n => format!("{n}\n"),
            })
            .collect();
        let diff = round_trip(&before, &after);
        assert_eq!(diff.matches("\n@@ ").count(), 2, "{diff}");
        assert!(diff.contains("@@ -1,6 +1,6 @@\n"), "{diff}");
        assert!(diff.contains("@@ -24,7 +24,6 @@\n"), "{diff}");
    }

    #[test]
    fn a_last_line_without_its_end_is_kept_so() {
        round_trip("a\nb", "a\nb\n");
        round_trip("a\nb\n", "a\nc");
        let diff = round_trip("x", "y");
        assert!(diff.contains("-x\n\\ No newline at end of file\n+y\n\\ No newline"));
    }

    #[test]
    fn a_file_made_or_emptied() {
        let made = unified("work/new.md", None, Some("hello\n"));
        assert_eq!(
            made,
            "--- /dev/null\n+++ b/work/new.md\n@@ -0,0 +1 @@\n+hello\n"
        );
        assert_eq!(reverse(&made, "hello\n").as_deref(), Some(""));
        round_trip("a\nb\n", "");
        round_trip("", "a\n");
        assert_eq!(unified("a", Some("same"), Some("same")), "");
    }

    #[test]
    fn a_file_changed_since_is_left() {
        let diff = unified("a", Some("a\nb\nc\n"), Some("a\nB\nc\n"));
        assert_eq!(
            reverse(&diff, "a\nB\nc\nd\n").as_deref(),
            Some("a\nb\nc\nd\n")
        );
        assert_eq!(reverse(&diff, "a\nb2\nc\n"), None);
        assert_eq!(reverse(&diff, ""), None);
        assert_eq!(reverse("not a diff", "a\n"), None);
    }

    #[test]
    fn many_edits_are_still_exact() {
        let before: String = (0..3000).map(|n| format!("line {n}\n")).collect();
        let after: String = (0..3000)
            .map(|n| format!("line {}\n", n * 7 % 3001))
            .collect();
        round_trip(&before, &after);
        let shuffled: String = (0..400).map(|n| format!("{}\n", (n * 13) % 97)).collect();
        let other: String = (0..400).map(|n| format!("{}\n", (n * 31) % 89)).collect();
        round_trip(&shuffled, &other);
    }
}
