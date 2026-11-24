//! Prepare protected state only when a run needs it, before untrusted code.

use crate::folders;
use link_format::{self as format, Path as Shown, Report, sentence};

/// Protect the control and installed-code roots before a run starts, then
/// collect abandoned staging from Safes. Read-only commands never call this.
pub fn prepare() -> anyhow::Result<()> {
    folders::need()?;
    // The mask must cover sockets created after this run starts too.
    link_files::create_private_dir(&link_files::layout::Root::Runtime.try_path()?)?;
    sweep_staging();
    Ok(())
}

/// Expired staging leaves Link's Safes, at most once a period. Best
/// effort: what the sweep can't read is left as it is and said aside,
/// and never stops the command the person asked for.
fn sweep_staging() {
    let swept = link_files::maintenance::now().and_then(link_files::maintenance::sweep);
    let because = |why: String| format::Prose::from(format::Sentence::adopt(&why));
    match swept {
        Ok(unswept) => {
            for left in unswept {
                format::aside(
                    Report::warning(sentence!(
                        "Link left {} as it was, removing expired staging from its Safes.",
                        Shown::new(&left.path)
                    ))
                    .why(because(left.error.to_string())),
                );
            }
        }
        Err(err) => format::aside(
            Report::warning(sentence!(
                "Link couldn't remove expired staging from its Safes."
            ))
            .why(because(format!("{err:#}"))),
        ),
    }
}
