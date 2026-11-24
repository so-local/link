//! The measure command's regression checks, in one test program.

link_test_kit::program!(system);

#[cfg(unix)]
mod runtime;
