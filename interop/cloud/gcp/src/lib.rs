//! The Google Cloud adapter, `lnk-gcp`: what of it a test reads.
//! The adapter itself is the program (`main.rs`).

pub mod flags;
pub mod price_catalog;

#[cfg(test)]
link_test_kit::program!(system);
