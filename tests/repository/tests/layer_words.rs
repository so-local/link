//! No layer names a concept of a layer built on it: each word a
//! `layer.toml` declares is said by no file of a layer below it, unless
//! `layer-words.toml` holds it, which may shrink, never grow (the rule is
//! `link_repository_check::layer_words`, which the private workspace runs
//! on its own roots too).

use std::path::Path;

#[test]
fn no_layer_names_a_concept_of_one_above_it() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let problems = link_repository_check::layer_words::check(&root, &here.join("layer-words.toml"));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
