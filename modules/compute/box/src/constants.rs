//! Box's names that TOML can't hold: each read where it's used by this
//! one name.

/// The variable `lnk box run` and `ssh` set to the name of the box here
/// they run in: `lnk box user` refuses there.
pub const HERE_BOX: &str = "LNK_BOX_HERE";

/// The folder in `~/Link/Boxes/<name>` that is a box here's home.
pub const HOME: &str = "home";
