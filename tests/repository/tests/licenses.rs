//! Link is MIT or Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, `license` in
//! the root `Cargo.toml`).

use std::path::Path;

#[test]
fn link_is_mit_or_apache_licensed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |file| std::fs::read_to_string(root.join(file)).unwrap();
    assert!(read("LICENSE-MIT").starts_with("MIT License"));
    assert!(read("LICENSE-APACHE").contains("Version 2.0, January 2004"));
    assert_eq!(env!("CARGO_PKG_LICENSE"), "MIT OR Apache-2.0");
}

/// Every license file the workspace tracks names its copyright holder and
/// years, the ones `[workspace.metadata.link.copyright]` declares, and
/// holds no placeholder left from a template.
#[test]
fn every_license_names_its_holder_and_no_placeholder() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let manifest: toml::Table =
        toml::from_str(&std::fs::read_to_string(root.join("Cargo.toml")).unwrap()).unwrap();
    let copyright = &manifest["workspace"]["metadata"]["link"]["copyright"];
    let (years, holder) = (
        copyright["years"].as_str().unwrap(),
        copyright["holder"].as_str().unwrap(),
    );
    // Apache's notice says "Copyright", MIT's "Copyright (c)".
    let notices = [
        format!("Copyright {years} {holder}"),
        format!("Copyright (c) {years} {holder}"),
    ];
    let mut problems = Vec::new();
    for file in link_repository_check::layer_words::tracked(&root) {
        let name = file.rsplit('/').next().unwrap_or(&file);
        if !(name == "LICENSE-MIT" || name == "LICENSE-APACHE") {
            continue;
        }
        let text = std::fs::read_to_string(root.join(&file)).unwrap();
        for placeholder in [
            "[yyyy]",
            "[name of copyright owner]",
            "<year>",
            "<copyright holders>",
        ] {
            if text.contains(placeholder) {
                problems.push(format!("{file} holds the placeholder {placeholder}"));
            }
        }
        if !notices.iter().any(|notice| text.contains(notice.as_str())) {
            problems.push(format!("{file} doesn't name {holder} for {years}"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
