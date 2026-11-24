//! Start and stand down a resolved roster without deleting its state.
use super::*;

pub(super) async fn run(out: &mut Out, named: String, start: bool) -> Result<Done, Report> {
    if remote::is_remote(&named) {
        return remote::lifecycle(&named, start);
    }
    let team = team_now(&named)?;
    let ids = by_id();
    let mut members = Vec::new();
    for member in team.members.values() {
        let name = ids.get(&member.id).within(
            "a member is away: move the whole team here before changing its local lifecycle",
        )?;
        members.push(here(name)?);
    }
    // Resolve every member before changing any service.
    if start {
        for agent in &members {
            agent.spec()?;
        }
    }
    let mut failed = Vec::new();
    for agent in &mut members {
        if start {
            crate::creating::start(out, agent, &cli::Starting::default()).await?;
        } else {
            if let Err(err) = crate::stopping::stop(agent) {
                failed.push(agent.name.clone());
                out.block(err);
            }
        }
    }
    if !failed.is_empty() {
        return Err(anyhow::anyhow!("could not stop these members: {}", failed.join(", ")).into());
    }
    Ok(Done::success(match start {
        true => sentence!("Team {} is started.", Name::new("team", &team.name)),
        false => sentence!(
            "Team {} is stopped; its agents and files are kept.",
            Name::new("team", &team.name)
        ),
    }))
}
