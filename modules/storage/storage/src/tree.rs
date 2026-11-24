//! `lnk storage list --tree`, drawn like Linux `tree`: folders nest with `├──`
//! and `└──`, `-L` limits the depth, and a folder shows what's under it
//! (its size, and per cloud the worst mark of its files).

use std::collections::BTreeMap;

use crate::files::human;
use crate::ops::{Here, Mark, Row};

#[derive(Default)]
struct Dir<'a> {
    /// Files and folders by name; tree sorts them together.
    entries: BTreeMap<&'a str, Entry<'a>>,
    /// What it holds, once [`summarize`]d.
    summary: Option<Summary>,
}

enum Entry<'a> {
    File(&'a Row),
    Dir(Dir<'a>),
}

/// What a folder holds, all the way down.
#[derive(Clone)]
struct Summary {
    size: u64,
    files: usize,
    dirs: usize,
    marks: Vec<Mark>,
    offloaded: usize,
    new: usize,
    changed: usize,
}

impl Summary {
    fn new(clouds: usize) -> Self {
        Summary {
            size: 0,
            files: 0,
            dirs: 0,
            marks: vec![Mark::Checked; clouds],
            offloaded: 0,
            new: 0,
            changed: 0,
        }
    }

    fn add_file(&mut self, row: &Row, first: bool) {
        self.size += row.size;
        self.files += 1;
        match row.here {
            Here::Offloaded => self.offloaded += 1,
            Here::New => self.new += 1,
            Here::Changed => self.changed += 1,
            Here::Here => {}
        }
        for (mark, (_, file)) in self.marks.iter_mut().zip(&row.clouds) {
            *mark = if first { *file } else { worse(*mark, *file) };
        }
    }

    fn add(&mut self, other: &Summary, first: bool) {
        self.size += other.size;
        self.files += other.files;
        self.dirs += other.dirs + 1;
        self.offloaded += other.offloaded;
        self.new += other.new;
        self.changed += other.changed;
        for (mark, theirs) in self.marks.iter_mut().zip(&other.marks) {
            *mark = if first {
                *theirs
            } else {
                worse(*mark, *theirs)
            };
        }
    }

    /// What a folder's line says about the files in it.
    fn note(&self) -> String {
        let mut parts = Vec::new();
        if self.offloaded > 0 && self.offloaded == self.files {
            parts.push("offloaded".to_string());
        } else if self.offloaded > 0 {
            parts.push(format!("{} offloaded", self.offloaded));
        }
        if self.changed > 0 {
            parts.push(format!("{} changed here", self.changed));
        }
        if self.new > 0 {
            parts.push(format!("{} not pushed", self.new));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("  ({})", parts.join(", "))
        }
    }
}

/// The less safe of two marks: a folder is only as backed up as its
/// least backed-up file. Archived and checked are both safe; a mix of
/// them shows as checked.
fn worse(a: Mark, b: Mark) -> Mark {
    let rank = |m: Mark| match m {
        Mark::Checked | Mark::Archived => 0,
        Mark::Unchecked => 1,
        Mark::Differs => 2,
        Mark::Missing => 3,
    };
    match rank(a).cmp(&rank(b)) {
        std::cmp::Ordering::Less => b,
        std::cmp::Ordering::Greater => a,
        std::cmp::Ordering::Equal if a == b => a,
        std::cmp::Ordering::Equal => Mark::Checked,
    }
}

/// What `dir` holds, kept on it and on each folder in it for [`draw`].
fn summarize(dir: &mut Dir, clouds: usize) -> Summary {
    let mut summary = Summary::new(clouds);
    let mut first = true;
    for entry in dir.entries.values_mut() {
        match entry {
            Entry::File(row) => summary.add_file(row, first),
            Entry::Dir(sub) => summary.add(&summarize(sub, clouds), first),
        }
        first = false;
    }
    dir.summary = Some(summary.clone());
    summary
}

/// Draws `rows` (paths relative to the bucket) under `label`. `prefix`
/// is the folder being shown, stripped from each path; `depth` limits
/// the levels drawn (`tree -L`); `dirs_only` draws only folders
/// (`tree -d`). `clouds` names the clouds, in the order of each row's
/// marks.
pub fn render(
    rows: &[Row],
    clouds: &[String],
    label: &str,
    prefix: &str,
    depth: Option<usize>,
    dirs_only: bool,
) -> String {
    let mut root = Dir::default();
    for row in rows {
        let relative = if prefix.is_empty() {
            row.path.as_str()
        } else {
            row.path
                .strip_prefix(prefix)
                .map(|rest| rest.trim_start_matches('/'))
                .filter(|rest| !rest.is_empty())
                // The prefix is the file itself.
                .unwrap_or_else(|| row.path.rsplit('/').next().unwrap_or(&row.path))
        };
        let mut dir = &mut root;
        let mut parts = relative.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                dir.entries.insert(part, Entry::File(row));
            } else {
                let entry = dir
                    .entries
                    .entry(part)
                    .or_insert_with(|| Entry::Dir(Dir::default()));
                // A file here and a folder of the same name in a cloud:
                // the folder wins.
                if matches!(entry, Entry::File(_)) {
                    *entry = Entry::Dir(Dir::default());
                }
                let Entry::Dir(sub) = entry else {
                    unreachable!()
                };
                dir = sub;
            }
        }
    }
    let total = summarize(&mut root, clouds.len());
    let mut out = String::new();
    let marks = |marks: &[Mark]| marks.iter().map(|m| m.symbol()).collect::<String>();
    out += &format!(
        "[{} {:>8}]  {label}{}\n",
        marks(&total.marks),
        human(total.size),
        total.note()
    );
    draw(&root, "", 1, depth, dirs_only, &mut out);
    out += &format!(
        "\n{} {}, {} {}, {}",
        total.dirs,
        if total.dirs == 1 {
            "directory"
        } else {
            "directories"
        },
        total.files,
        if total.files == 1 { "file" } else { "files" },
        human(total.size)
    );
    if total.offloaded > 0 {
        out += &format!("; {} offloaded", total.offloaded);
    }
    out += "\n";
    if !clouds.is_empty() {
        out += &format!(
            "One mark per cloud ({}): ✓ checked  A checked, archive tier  ? not checked (seen by tree --refresh)  ≠ differs  ✗ missing\n",
            clouds.join(", ")
        );
    }
    out
}

fn draw(
    dir: &Dir,
    indent: &str,
    level: usize,
    depth: Option<usize>,
    dirs_only: bool,
    out: &mut String,
) {
    if depth.is_some_and(|d| level > d) {
        return;
    }
    let shown: Vec<_> = dir
        .entries
        .iter()
        .filter(|(_, e)| !dirs_only || matches!(e, Entry::Dir(_)))
        .collect();
    for (i, (name, entry)) in shown.iter().enumerate() {
        let last = i + 1 == shown.len();
        let branch = if last { "└── " } else { "├── " };
        let marks =
            |marks: &mut dyn Iterator<Item = Mark>| marks.map(|m| m.symbol()).collect::<String>();
        match entry {
            Entry::File(row) => {
                let note = match row.here {
                    Here::Here => "",
                    Here::Changed => "  (changed here since pushed)",
                    Here::New => "  (not pushed)",
                    Here::Offloaded => "  (offloaded)",
                };
                *out += &format!(
                    "{indent}{branch}[{} {:>8}]  {name}{note}\n",
                    marks(&mut row.clouds.iter().map(|(_, m)| *m)),
                    human(row.size)
                );
            }
            Entry::Dir(sub) => {
                let summary = sub.summary.as_ref().expect("summarized before drawing");
                *out += &format!(
                    "{indent}{branch}[{} {:>8}]  {name}/{}\n",
                    marks(&mut summary.marks.iter().copied()),
                    human(summary.size),
                    summary.note()
                );
                let deeper = format!("{indent}{}", if last { "    " } else { "│   " });
                draw(sub, &deeper, level + 1, depth, dirs_only, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str, size: u64, here: Here, marks: &[Mark]) -> Row {
        Row {
            path: path.into(),
            size,
            here,
            clouds: marks.iter().map(|m| ("c".to_string(), *m)).collect(),
        }
    }

    fn rows() -> Vec<Row> {
        use Mark::*;
        vec![
            row("notes.txt", 2, Here::New, &[Missing, Missing]),
            row(
                "photos/2019/a.jpg",
                4000,
                Here::Offloaded,
                &[Checked, Archived],
            ),
            row("photos/2019/b.jpg", 5000, Here::Here, &[Checked, Checked]),
            row("photos/2020/c.jpg", 6000, Here::Here, &[Checked, Unchecked]),
        ]
    }

    fn clouds() -> Vec<String> {
        vec!["aws".into(), "gcp".into()]
    }

    #[test]
    fn draws_like_tree() {
        let text = render(&rows(), &clouds(), "~/Link/Files", "", None, false);
        let expected = "\
[✗✗  15.0 KB]  ~/Link/Files  (1 offloaded, 1 not pushed)
├── [✗✗      2 B]  notes.txt  (not pushed)
└── [✓?  15.0 KB]  photos/  (1 offloaded)
    ├── [✓✓   9.0 KB]  2019/  (1 offloaded)
    │   ├── [✓A   4.0 KB]  a.jpg  (offloaded)
    │   └── [✓✓   5.0 KB]  b.jpg
    └── [✓?   6.0 KB]  2020/
        └── [✓?   6.0 KB]  c.jpg

3 directories, 4 files, 15.0 KB; 1 offloaded
One mark per cloud (aws, gcp): ✓ checked  A checked, archive tier  ? not checked (seen by tree --refresh)  ≠ differs  ✗ missing
";
        assert_eq!(text, expected);
    }

    #[test]
    fn limits_the_depth() {
        let text = render(&rows(), &clouds(), "~/Link/Files", "", Some(1), false);
        assert!(text.contains("└── [✓?  15.0 KB]  photos/  (1 offloaded)\n\n"));
        assert!(!text.contains("2019"));
        // The totals still count everything.
        assert!(text.contains("3 directories, 4 files"));
    }

    #[test]
    fn folders_only_and_a_prefix() {
        let text = render(&rows(), &clouds(), "photos", "photos", None, true);
        assert!(text.contains("├── [✓✓   9.0 KB]  2019/"));
        assert!(!text.contains("a.jpg"));
        assert!(!text.contains("notes.txt"));
        let rows: Vec<Row> = rows()
            .into_iter()
            .filter(|r| r.path == "notes.txt")
            .collect();
        let text = render(&rows, &clouds(), "notes.txt", "notes.txt", None, false);
        assert!(text.contains("└── [✗✗      2 B]  notes.txt  (not pushed)"));
    }

    #[test]
    fn a_folder_is_as_safe_as_its_worst_file() {
        use Mark::*;
        assert_eq!(worse(Checked, Archived), Checked);
        assert_eq!(worse(Archived, Archived), Archived);
        assert_eq!(worse(Checked, Missing), Missing);
        assert_eq!(worse(Differs, Unchecked), Differs);
    }
}
