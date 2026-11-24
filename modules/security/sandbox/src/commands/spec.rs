//! `lnk sandbox spec show`: the spec a run would use.

use link_format::{Done, Out, Report, WithinExt};
use link_sandbox::opening::Opening;
use link_sandbox::spec;
use link_sandbox_client::{At, Confinement, ProxyAt, Way};

use super::*;

/// `lnk sandbox spec show`: the spec a file, `--spec`, and its field
/// flags make, as a run reads it, its folders absolute: a document for a
/// file, or with `--json` the same as JSON. A sandbox names no thing.
pub(crate) fn show(
    out: &mut Out,
    show: &link_spec::Show,
    fields: link_spec::FieldFlags,
) -> Result<Done, Report> {
    if show.thing.is_some() {
        return Err(
            anyhow::anyhow!("a sandbox spec names no thing; give --spec FILE or flags").into(),
        );
    }
    let opening = Opening::of(show.spec.clone(), fields, None, false);
    let (spec, read) = opening.spec()?;
    let dir = match &read {
        Some(read) => read.dir.clone(),
        None => std::env::current_dir().within("finding this folder")?,
    };
    let home = link_files::try_home_dir()?;
    // Checked as a run would, but for finding its programs,
    // which run where it's run; then printed absolute.
    let confinement = Confinement::of(
        &spec,
        At {
            name: "spec",
            dir: &dir,
            home: &home,
            path: None,
            way: Way::Proxy(ProxyAt::default()),
        },
    )?;
    spec::network_here(&confinement, spec::on_cloud())?;
    let spec = spec.absolute(&dir, &home)?;
    match show.output.json {
        true => {
            let json = <spec::Spec as link_spec::Specification>::format().print_json(&spec)?;
            out.reply(&serde_json::from_slice::<serde_json::Value>(&json).within("its JSON")?);
        }
        false => out.data(link_spec::print_spec(&spec)?.as_bytes()),
    }
    // The spec printed is this OS's: the file's tables for another are
    // said, never printed as if they were this one's.
    let left = read
        .as_ref()
        .map(|read| read.spec.others_than(spec::Os::here()))
        .unwrap_or_default();
    if !left.is_empty() {
        out.block(link_format::Note(link_format::Sentence::adopt(&format!(
            "This is the spec on this OS; the file's {} apply on the other and aren't printed.",
            left.join(", ")
        ))));
    }
    Ok(Done::shown())
}
