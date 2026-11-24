//! A team made and applied again as its spec says (`lnk agent team
//! create --spec`): its shared settings and routing, here or by a cloud
//! account.
use super::*;

/// `lnk agent team create --spec <file>`: the team as its spec, with its
/// field flags over it, says, made or applied again, and the team file
/// it keeps printed.
pub(super) async fn apply(
    out: &mut Out,
    named: Option<&str>,
    path: &std::path::Path,
    fields: &link_spec::FieldFlags,
    cloud: Option<&str>,
) -> Result<Done, Report> {
    let mut read = link_spec::read_spec::<link_agent_spec::TeamFile>(path)?;
    if !fields.is_empty() {
        let made = link_spec::read_with::<link_agent_spec::TeamFile>(Some(path), fields)?;
        read.text = link_spec::print_spec(&made.spec)?;
        read.spec = made.spec;
    }
    if let Some(named) = named
        && named != read.spec.name
    {
        return Err(Report::error(sentence!(
            "The spec is team {}'s, not {}'s.",
            Name::new("team", &read.spec.name),
            Name::new("team", named)
        ))
        .exit(Exit::Usage));
    }
    let current = link_agent_spec::team_document(&read.text)?;
    let current = match cloud {
        Some(cloud) => remote::apply(cloud, &current)?.file,
        None => {
            let applied = defining::apply(out, &read)?;
            restart(out, &applied.affected).await?;
            applied.text
        }
    };
    out.data(current.as_bytes());
    Ok(Done::shown())
}
