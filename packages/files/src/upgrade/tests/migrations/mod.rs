//! The coded kind's code for the transfer that names it.

use crate::schema::{Migration, Migrations};

mod coded_from_setting;

impl Migrations for super::Coded {
    const ALL: &'static [Migration] = &[Migration {
        name: "upgrade coded_from_upgrade setting_1",
        run: coded_from_setting::run,
    }];
}
