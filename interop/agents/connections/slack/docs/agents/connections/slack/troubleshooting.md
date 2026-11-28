# Troubleshooting

What to do when your agent's [Slack app](README.md) doesn't connect, or its messages
don't arrive.

## "<harness> doesn't speak Slack"

The harness carries no Slack. Switch to the harness the message names,
which speaks it.

```sh
lnk agent harness set <harness>
```

## "Sending messages to this app has been turned off"

Slack shows this in the app's messages when its Messages Tab is off. On
the app's page, under App Home, turn on the Messages Tab.

## `connect slack` refuses the app-level token

The app-level token needs the `connections:write` scope, and the app
needs Socket Mode. Give the token that scope and turn on Socket Mode on
the app's page, then connect again.

## The pairing code gets no response

Keep [`lnk agent connect slack`](../../../../../../../../modules/agents/agent/docs/agents/connections/connect/README.md#choose-an-app) running while sending the code in the
app's Messages tab. Link listens in Socket Mode before showing the
code and displays that it is waiting. If Slack closes the connection,
pairing fails with a reason; if no matching DM arrives, the code expires
after ten minutes.

### Pairing progress

The pairing terminal reports when Slack's Socket Mode greeting arrives,
when an event is unsuitable for pairing, when a direct message does not
match the code, and when the code matches and confirmation begins. These
messages contain no token, code or message text. If it stays connected
without reporting a DM after you send one, check that both tokens belong
to the app you opened and that its `message.im` subscription is installed.
Pairing progress is in this terminal, rather than [`lnk agent log show`](../../../../../../../../modules/agents/agent/docs/agents/start/log/README.md#read-the-log).

### App subscriptions

On the app's settings page, check Socket Mode, the Messages Tab and the
`message.im` bot event subscription. If you change event subscriptions
or OAuth scopes, reinstall the app in your workspace, then connect
again. Each agent needs its own Slack app; another Socket Mode client
for the same app can receive events intended for this pairing attempt.

## Pushed messages don't arrive

Link couldn't switch the app to pushes. Take the two steps it gave
(Socket Mode off, then the Request URL), or give `hook` a new
configuration token, since one lasts 12 hours.
