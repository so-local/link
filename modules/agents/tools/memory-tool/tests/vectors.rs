//! Search by meaning over binary codes and small vectors: the true
//! nearest among the first ten in nearly every query, reading no more
//! than the best few hundred's small vectors beside the codes it maps.

use std::sync::Arc;

use link_memory_tool::Index;
use link_memory_tool::semantic::Embedder;

/// Dimensions of the generated vectors: the default model's, before
/// memory keeps 256 of them.
const DIMENSIONS: usize = 1024;

/// A number's vector: noise of its own, the same each time.
fn noise(seed: u64) -> Vec<f32> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..DIMENSIONS)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 2001) as f32 / 1000.0 - 1.0
        })
        .collect()
}

fn unit(vector: Vec<f32>) -> Vec<f32> {
    link_memory_tool::semantic::normalized(vector)
}

/// Generated vectors, not trained to be cut: note `n<i>`'s is its own
/// noise, and `query <i>` is near note `i`'s, with noise of its own.
struct Generated;

impl Embedder for Generated {
    fn model(&self) -> &str {
        "generated"
    }

    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| {
                let number = |prefix: &str| -> Option<u64> {
                    let rest = text.strip_prefix(prefix)?;
                    rest.split(|c: char| !c.is_ascii_digit())
                        .next()?
                        .parse()
                        .ok()
                };
                match (number("query "), number("n")) {
                    (Some(n), _) => {
                        let (near, own) = (unit(noise(n)), unit(noise(n + 1_000_000)));
                        unit(
                            near.iter()
                                .zip(own)
                                .map(|(a, b)| 0.6 * a + 0.8 * b)
                                .collect(),
                        )
                    }
                    (None, Some(n)) => unit(noise(n)),
                    _ => unit(noise(u64::MAX)),
                }
            })
            .collect())
    }
}

#[test]
fn the_true_nearest_is_found_from_codes_and_small_vectors() {
    let root = link_test_kit::TempDir::new("vectors-nearest");
    let file = root.join(".index.sqlite");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    const NOTES: u64 = 3000;
    for n in 0..NOTES {
        std::fs::write(root.join(format!("memory/n{n}.md")), "An item.\n").unwrap();
    }
    let mut index = Index::open(&root, Some(&file)).unwrap();
    index.refresh().unwrap();
    index.embed_with(Arc::new(Generated)).unwrap();
    while index.embed_pending(500).unwrap() > 0 {}
    // Kept as codes and small vectors, never in the index.
    let codes = std::fs::metadata(root.join(".index.sqlite.codes"))
        .unwrap()
        .len();
    assert_eq!(codes, NOTES * 256 / 8);

    let queries = 50;
    let mut found = 0;
    for q in 0..queries {
        let n = q * 59 % NOTES;
        let hits = index.search(&format!("query {n}"), None, 10).unwrap();
        found += usize::from(hits.iter().any(|hit| hit.path == format!("memory/n{n}.md")));
    }
    assert!(found >= 48, "{found} of {queries} found the true nearest");
    // Beside the codes it maps, each search read only the small vectors
    // of those it rescored.
    let slot = 16 + 256;
    let most = queries * link_memory_tool::limits::limits().rescore as u64 * slot;
    assert!(
        index.small_read() <= most,
        "{} bytes read",
        index.small_read()
    );
    assert!(index.small_read() > 0);
}
