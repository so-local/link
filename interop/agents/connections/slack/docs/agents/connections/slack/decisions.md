# Decisions

Why the Slack module gives each agent an app of its own, and how it
hears messages.

## Why one Slack app per agent, in Socket Mode?

Slack shares a Socket Mode app's messages among whatever connects, so
two agents on one app would each miss some. Each app is made from Link's
manifest, and `connect` refuses an app another agent here, or running on
a box, has. Socket Mode, because a public address for each agent would
mean a tunnel just for Slack.

### Where Slack pushes

On a machine that packs agents, which has a public address and runs each
agent only while it answers, nothing can hold a socket for each: there
the app is switched to a request URL and [Slack pushes its events by the
Events API](guide.md#when-slack-pushes-its-messages), signed with the app's signing secret. At home it stays in
Socket Mode, which needs no public address.

## How does the Slack bridge hear messages?

In Socket Mode, with the app-level token [`lnk agent connect slack`](../../../../../../../../modules/agents/agent/docs/agents/README.md#connect-your-channels)
already asks for: the module opens the connection, so the user needs no
public address and Link runs no web server for Slack. Each event is
acknowledged at once, before the answer is written, so Slack doesn't
send it again; one it sends again anyway is carried once. A direct
conversation is `slack:<channel id>`, and a thread adds its timestamp
as `slack:<channel id>.<thread timestamp>`. Answers return to that
conversation. Discord's Gateway is the same kind of connection.
