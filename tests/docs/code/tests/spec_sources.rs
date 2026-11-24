//! The public source registration inventory is paired with actual typed kinds.
link_test_kit::program!(unit);

#[test]
fn every_public_registered_kind_has_source_located_typed_fields() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let tree = link_docs_check::Tree::read(&root, false, &root.join("tests/docs/large-files.toml"));
    let files = tree
        .files
        .iter()
        .map(|(path, text)| {
            (
                std::path::PathBuf::from("link-public")
                    .join(path.strip_prefix(&tree.root).unwrap()),
                text.clone(),
            )
        })
        .collect();
    let kinds: Vec<_> = link_docs_check_code::references::kinds::kinds()
        .into_iter()
        .map(|page| page.kind)
        .collect();
    let found = link_docs_check::spec_sources::discover(&files, &kinds);
    assert!(found.problems.is_empty(), "{}", found.problems.join("\n"));
    // A version 1 module's kind is registered by its declaration.
    assert!(
        found
            .fields
            .iter()
            .any(|field| field.source.ends_with("modules/tooling/bundle/types.toml")),
        "bundle's kind is found in its types.toml"
    );
    let owners: std::collections::BTreeSet<_> =
        found.fields.iter().map(|field| &field.owner).collect();
    assert_eq!(
        owners.len(),
        kinds.len(),
        "each actual kind has its own source owner"
    );
    assert!(
        found
            .fields
            .iter()
            .all(|field| !field.content["nodes"].as_array().unwrap().is_empty())
    );
}
