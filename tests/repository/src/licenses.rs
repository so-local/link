//! Link is MIT or Apache-2.0, and the programs it releases link every crate
//! of its lock file: a crate whose license would put conditions on them
//! that those two don't fails, so the choice is made on purpose, never by
//! a `cargo update`.

use std::path::Path;

/// Permissive licenses: their conditions are keeping notices, not sharing
/// source. An expression passes when one of its `OR` choices uses only these.
pub const ALLOWED: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "Unlicense",
    "BSL-1.0",
    "CC0-1.0",
    "0BSD",
    // MIT without even its notice (borrow-or-share).
    "MIT-0",
    // webpki-roots: Mozilla's root certificates, as data.
    "CDLA-Permissive-2.0",
];

/// Each crate from a registry whose license isn't permissive, or isn't
/// said, read from `cargo metadata` (its lock, every platform: a crate of
/// another platform is downloaded once, as a build would).
pub fn licenses(root: &Path) -> Vec<String> {
    let output = link_test_kit::cargo()
        .current_dir(root)
        .args(["metadata", "--locked", "--format-version", "1"])
        .output()
        .expect("cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).expect("its JSON");
    let mut problems = Vec::new();
    for package in metadata["packages"].as_array().expect("packages") {
        if package["source"].is_null() {
            continue;
        }
        let name = format!(
            "{} {}",
            package["name"].as_str().unwrap_or("?"),
            package["version"].as_str().unwrap_or("?")
        );
        match package["license"].as_str() {
            Some(license) if permitted(license) => {}
            Some(license) => problems.push(format!("{name}: {license} isn't permissive")),
            None => problems.push(format!("{name}: says no license (only a file)")),
        }
    }
    problems
}

/// Whether an SPDX expression is permitted: `OR` binds looser than `AND`,
/// `WITH` adds an exception (which only loosens a license), `/` is the old
/// `OR`.
pub fn permitted(expr: &str) -> bool {
    let spaced = expr
        .replace('/', " OR ")
        .replace('(', " ( ")
        .replace(')', " ) ");
    let tokens: Vec<&str> = spaced.split_whitespace().collect();
    let mut pos = 0;
    let ok = or(&tokens, &mut pos);
    ok && pos == tokens.len()
}

fn or(tokens: &[&str], pos: &mut usize) -> bool {
    let mut ok = and(tokens, pos);
    while tokens.get(*pos) == Some(&"OR") {
        *pos += 1;
        ok |= and(tokens, pos);
    }
    ok
}

fn and(tokens: &[&str], pos: &mut usize) -> bool {
    let mut ok = one(tokens, pos);
    while tokens.get(*pos) == Some(&"AND") {
        *pos += 1;
        ok &= one(tokens, pos);
    }
    ok
}

fn one(tokens: &[&str], pos: &mut usize) -> bool {
    let token = tokens.get(*pos).copied().unwrap_or("");
    *pos += 1;
    let ok = if token == "(" {
        let ok = or(tokens, pos);
        *pos += 1; // ")"
        ok
    } else {
        ALLOWED.contains(&token)
    };
    if tokens.get(*pos) == Some(&"WITH") {
        *pos += 2;
    }
    ok
}

/// Each npm package of the website's lock file (`web/package-lock.json`)
/// whose license isn't permissive, or isn't said, and isn't in `accepted`
/// (`npm-licenses.toml`: a package's name, then why); and each accepted
/// name no package has any more.
pub fn npm_licenses(root: &Path, accepted: &Path) -> Vec<String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Accepted {
        accepted: std::collections::BTreeMap<String, String>,
    }
    let lock: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("web/package-lock.json")).expect("the website's lock"),
    )
    .expect("its JSON");
    let accepted: Accepted =
        toml::from_str(&std::fs::read_to_string(accepted).expect("npm-licenses.toml"))
            .expect("a valid npm-licenses.toml");
    let mut used = std::collections::BTreeSet::new();
    let mut problems = Vec::new();
    for (path, package) in lock["packages"].as_object().expect("packages") {
        if path.is_empty() || package["link"] == true {
            continue;
        }
        // The package's own name: its folder under its last `node_modules/`.
        let name = path.rsplit("node_modules/").next().unwrap_or(path);
        match package["license"].as_str() {
            Some(license) if permitted(license) => {}
            _ if accepted.accepted.contains_key(name) => {
                used.insert(name.to_owned());
            }
            Some(license) => problems.push(format!("{path}: {license} isn't permissive")),
            None => problems.push(format!("{path}: says no license")),
        }
    }
    for name in accepted
        .accepted
        .keys()
        .filter(|name| !used.contains(*name))
    {
        problems.push(format!(
            "{name} is accepted but no package needs it: take it off"
        ));
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::permitted;

    #[test]
    fn license_expressions() {
        assert!(permitted("MIT OR Apache-2.0"));
        assert!(permitted("MIT/Apache-2.0"));
        assert!(permitted("(MIT OR Apache-2.0) AND Unicode-3.0"));
        assert!(permitted("Apache-2.0 WITH LLVM-exception OR MIT"));
        assert!(permitted("MIT OR Apache-2.0 OR LGPL-2.1-or-later"));
        assert!(!permitted("GPL-3.0"));
        assert!(!permitted("MIT AND GPL-3.0"));
        assert!(!permitted("(MIT OR GPL-3.0) AND LGPL-2.1"));
    }
}
