//! The index keeps passages and where they are, never the text: an edit
//! rewrites only the passages it touched, and the index is a fraction of
//! what it indexes.

use std::collections::BTreeSet;
use std::path::Path;

use link_memory_tool::Index;

/// Generated Markdown as the plan measured it: a heading every 40 lines,
/// paragraphs of three to seven lines, words drawn by Zipf's law from a
/// made-up vocabulary, about 60 bytes a line. The same for each seed.
pub(crate) struct Corpus {
    words: Vec<String>,
    /// Each word's share, summed: a draw is a search in it.
    cumulative: Vec<f64>,
    state: u64,
}

impl Corpus {
    pub(crate) fn new(seed: u64) -> Self {
        const SYLLABLES: &[&str] = &[
            "ka", "lo", "mi", "ne", "ru", "ta", "vo", "si", "pe", "da", "go", "fu", "ri", "an",
            "el", "or", "un", "is", "ba", "co",
        ];
        let mut corpus = Self {
            words: Vec::new(),
            cumulative: Vec::new(),
            state: seed | 1,
        };
        let mut sum = 0.0;
        for rank in 0..20_000usize {
            let mut word = String::new();
            let mut n = rank;
            loop {
                word.push_str(SYLLABLES[n % SYLLABLES.len()]);
                n /= SYLLABLES.len();
                if n == 0 {
                    break;
                }
            }
            corpus.words.push(word);
            sum += 1.0 / ((rank + 1) as f64).powf(1.07);
            corpus.cumulative.push(sum);
        }
        corpus
    }

    /// The next number, xorshift.
    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    fn word(&mut self) -> String {
        let total = *self.cumulative.last().expect("words");
        let at = (self.next() % 1_000_000) as f64 / 1_000_000.0 * total;
        let i = self.cumulative.partition_point(|c| *c < at);
        self.words[i.min(self.words.len() - 1)].clone()
    }

    /// A document of `lines` lines.
    pub(crate) fn document(&mut self, lines: usize) -> String {
        let mut text = String::new();
        let mut n = 0;
        while n < lines {
            if n % 40 == 0 {
                text.push_str(&format!("## {} {}\n\n", self.word(), self.word()));
                n += 2;
                continue;
            }
            let paragraph = 3 + (self.next() % 5) as usize;
            for _ in 0..paragraph.min(lines - n) {
                let mut line = String::new();
                while line.len() < 56 {
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    let word = self.word();
                    line.push_str(&word);
                }
                text.push_str(&line);
                text.push('\n');
                n += 1;
            }
            text.push('\n');
            n += 1;
        }
        text
    }
}

/// Each passage's id, of the document at `path` in the index `file`.
fn passages(file: &Path, path: &str) -> BTreeSet<i64> {
    let db = rusqlite::Connection::open(file).unwrap();
    let mut statement = db
        .prepare(
            "SELECT p.id FROM passages p JOIN documents d ON d.id = p.document
             WHERE d.path = ?1",
        )
        .unwrap();
    statement
        .query_map([path], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// One line changed in a 5,000-line file: one passage is written again,
/// every other kept as it was, moved to where it is now, and the file is
/// found by the new line's word.
#[test]
fn a_one_line_edit_rewrites_one_passage() {
    let root = link_test_kit::TempDir::new("passages-edit");
    let file = root.join(".index.sqlite");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    let long = root.join("memory/long.md");
    let text = Corpus::new(7).document(5000);
    std::fs::write(&long, &text).unwrap();
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    let before = passages(&file, "memory/long.md");
    assert!(before.len() > 100, "{} passages", before.len());

    // A line in the middle, longer by a word: every passage after it moves.
    let lines: Vec<&str> = text.lines().collect();
    let middle = (2500..lines.len())
        .find(|n| lines[*n].len() > 20 && !lines[*n].starts_with('#'))
        .unwrap();
    let mut edited: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    edited[middle].push_str(" zanzibar");
    std::fs::write(&long, edited.join("\n") + "\n").unwrap();
    assert_eq!(index.refresh().unwrap().indexed, 1);
    let after = passages(&file, "memory/long.md");
    let gone: Vec<_> = before.difference(&after).collect();
    let new: Vec<_> = after.difference(&before).collect();
    assert_eq!((gone.len(), new.len()), (1, 1), "{gone:?} {new:?}");

    let hits = index.search("zanzibar", None, 10).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].line, middle + 1);
    assert!(
        hits[0].snippet.contains("**zanzibar**"),
        "{}",
        hits[0].snippet
    );
    // A later passage's line, read from the file where the index says.
    let read = index
        .read("memory/long.md", middle + 1, Some(1), None)
        .unwrap()
        .unwrap();
    assert!(read.text.ends_with(" zanzibar"), "{}", read.text);
}

/// The index of a generated corpus, its words and passages, is under 0.6
/// times the corpus: the text stays in the files.
#[test]
fn the_index_is_a_fraction_of_the_corpus() {
    let root = link_test_kit::TempDir::new("passages-size");
    let file = root.join(".index.sqlite");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    let mut corpus = Corpus::new(11);
    let mut size = 0;
    for n in 0..12 {
        let text = corpus.document(5000);
        size += text.len() as u64;
        std::fs::write(root.join(format!("memory/{n}.md")), text).unwrap();
    }
    let mut index = Index::open(&root, Some(&file)).unwrap();
    assert_eq!(index.refresh().unwrap().indexed, 12);
    // Closed, its log checkpointed into it.
    drop(index);
    let mut indexed = 0;
    for end in ["", "-wal"] {
        let mut path = file.clone().into_os_string();
        path.push(end);
        indexed += std::fs::metadata(path).map_or(0, |m| m.len());
    }
    // Its words, in their folder beside it.
    for entry in std::fs::read_dir(root.join(".index.sqlite.words")).unwrap() {
        indexed += entry.unwrap().metadata().unwrap().len();
    }
    let share = indexed as f64 / size as f64;
    assert!(share < 0.6, "the index is {share:.2} of the corpus");
}
