//! The disks kind's code for the schema steps that name it, one module a
//! step, each named for the kind and its two schemas.

use crate::schema::{Migration, Migrations};

mod disks_1_to_2;
#[cfg(test)]
mod tests;

impl Migrations for super::Disks {
    const ALL: &'static [Migration] = &[Migration {
        name: "disks_1_to_2",
        run: disks_1_to_2::run,
    }];
}
