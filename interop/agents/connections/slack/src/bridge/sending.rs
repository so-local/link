//! Owner-authorized outbound delivery and its platform message identities.
use super::*;

pub async fn send(
    bridge: Bridge,
    chat: String,
    text: String,
) -> anyhow::Result<connection::SentMessages> {
    let settings: link_harness_contract::harness::Slack = bridge
        .settings
        .expose()
        .read()
        .context("reading the Slack settings on stdin")?;
    let (channel, in_thread) = place(&chat).unwrap_or_default();
    let direct = in_thread.is_none() && settings.dm_channel.as_deref() == Some(channel);
    let ours = !channel.is_empty()
        && (direct
            || (bridge.answers_groups()
                && (connection::knows_group(&bridge.state, channel)
                    || bridge.shared.iter().any(|c| c == channel))));
    anyhow::ensure!(
        ours,
        "Slack conversation {chat} isn't the owner's direct one with the bot, nor a channel they talk to the agent in: not sending there"
    );
    let bot = Bot::new(settings.bot_token.duplicate());
    let mut messages = Vec::new();
    for piece in format::pieces(&text) {
        messages.push(
            bot.post(channel, in_thread, &piece)
                .await
                .context("Slack didn't take the message")?
                .context("Slack didn't take the message")?,
        );
    }
    Ok(connection::SentMessages { messages })
}
