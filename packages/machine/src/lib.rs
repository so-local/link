//! This installation and whose it is: its ID and name ([`installation`]);
//! the machine it runs on, a box or your computer ([`machine`]); how apt
//! fetches on one Link sets up ([`apt`]); and the rules every name and ID
//! ([`names`]) and every owner ([`owner`]) Link makes follows.

pub mod apt;
pub mod installation;
pub mod machine;
pub mod names;
pub mod owner;

#[cfg(test)]
link_test_kit::program!(unit);
