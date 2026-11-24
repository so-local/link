//! Documentation checked against released parsers, types and owning declarations.

pub mod inventories;

pub mod coverage;

pub mod owner_links;

pub mod reviews;

pub mod files;

pub mod flags;

pub mod environment;

pub mod json;

pub mod references;

pub mod parsers;

pub mod arguments;

mod syntax;

#[cfg(test)]
link_test_kit::program!(system);
