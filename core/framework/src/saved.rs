//! The files a program writes, which `lnk upgrade` asks it about: what
//! it will change in a release's upgrade, then its transfers and
//! conversions (`link_files::upgrade`). Its `main` names the formats it
//! owns with [`own`] before it describes itself, and the discovery
//! request `--link-upgrade <stage>` is answered before any operational
//! startup, in the protocol version its asker states, reading what the
//! asker gave on standard input.

use std::io::Read;

use link_files::upgrade::{Saved, Stage};
use link_format::WithinExt;

/// The formats this program owns, as its `main` gave them to [`own`].
static OWNED: crate::named::Named<Saved> = crate::named::Named::new();

/// Name the formats of the files this program writes, each
/// `|| Saved::of::<T>()`: what an upgrade converts, offers and moves.
/// Each is made only when an upgrade asks, never at a command's start.
pub fn own(formats: &[fn() -> Saved]) {
    OWNED.name(formats);
}

/// Answer `--link-upgrade <stage>` for the formats this program owns,
/// none where it named none, and exit.
pub fn answer(stage: Stage) -> ! {
    let mut out = link_format::Out::with(link_settings::output::machine());
    let owned = OWNED.get();
    let result = link_files::upgrade::asked()
        .and_then(|asked| {
            // Only an asker stating its version gives a request.
            let mut request = Vec::new();
            if asked.is_some() {
                std::io::stdin().read_to_end(&mut request)?;
            }
            let roots = link_files::upgrade::roots()?;
            link_files::upgrade::respond(stage, owned, &roots, asked, &request)
        })
        .and_then(|answer| {
            let mut document = serde_json::to_vec(&answer)?;
            document.push(b'\n');
            out.data(&document);
            Ok(link_format::Done::shown())
        })
        .within(format!("the upgrade's {}", stage.word()))
        .map_err(link_format::Report::from);
    std::process::exit(i32::from(out.finish(result)));
}
