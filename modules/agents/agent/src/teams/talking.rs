//! Owner messages reuse personal-agent chat after resolving the team target.
//! One message follows the team as it moves ([`crate::following`]).
use super::*;
use crate::following::Found;

pub(super) async fn run(out: &mut Out, args: cli::ChatSendArgs) -> Result<Done, Report> {
    let mut chat = cli::Chat::team(&args);
    let team = team_of(&args);
    if chat.live {
        return once(out, &args, &chat).await.and_then(|found| match found {
            Found::Done(done) => Ok(done),
            Found::Moving(why) => Err(Report::error(sentence!("{}.", why))),
        });
    }
    if chat.chat.is_none() {
        // The team's thread, from its ID: the same wherever it runs.
        let named = cli::hosted_name(team, args.cloud.as_deref());
        let team = if remote::is_remote(&named) {
            let (account, hosted) = remote::find(&named)?;
            let (kept, _) = remote::read(&account, &hosted.id)?;
            link_agent_spec::parse_team(&kept.file)?
        } else {
            team_now(&named)?
        };
        chat.chat = Some(format!("team-{}", team.id));
    }
    let args = &args;
    crate::following::team(out, team, &chat, async |out, chat| {
        once(out, args, chat).await
    })
    .await
}

/// One try of `chat`, the message of `args`: to the member where the team
/// is now, or why it's not taken yet.
async fn once(out: &mut Out, args: &cli::ChatSendArgs, chat: &cli::Chat) -> Result<Found, Report> {
    let named = cli::hosted_name(team_of(args), args.cloud.as_deref());
    let (team, hosted) = if remote::is_remote(&named) {
        let (account, hosted) = remote::find(&named)?;
        let (kept, _) = remote::read(&account, &hosted.id)?;
        (link_agent_spec::parse_team(&kept.file)?, Some(account))
    } else {
        (team_now(&named)?, None)
    };
    let (member, entry) = team.target(args.member.as_deref())?;
    if let Some(account) = hosted {
        let talked = link_agent::chat::Chat::hosted_team(
            account,
            team.id.clone(),
            args.member.clone(),
            chat.thread()?,
        );
        return Ok(
            match crate::conversations::talk_or_later(out, talked, member, chat).await? {
                Ok(done) => Found::Done(done),
                Err(later) => Found::Moving(later.why),
            },
        );
    }
    entry
        .owner
        .is_none()
        .then_some(())
        .within("an outside member is reached through its hosting transport")?;
    let mut names = settings::names()?
        .into_iter()
        .filter(|name| settings::key_of(name) == entry.id);
    let name = names
        .next()
        .within("the selected member is not on this machine")?;
    names
        .next()
        .is_none()
        .then_some(())
        .within("more than one local agent has the selected member's ID")?;
    crate::following::found(out, &name, chat).await
}

/// The team `lnk agent chat send --team` messages: [`super::owns`] takes
/// only a send that names one.
fn team_of(args: &cli::ChatSendArgs) -> &str {
    args.team.as_deref().expect("a team's send names its team")
}
