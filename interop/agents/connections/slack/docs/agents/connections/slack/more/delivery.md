# Delivery Protocol

The Slack adapter’s delivery commands connect a shared hosting process to its native service. These commands are run by Link; the normal connection workflow stays in the [guide](../guide.md).

## When Slack pushes its messages

At home, the app runs in Socket Mode: Link connects out to Slack, and
nothing on your machine listens. On a machine that packs agents, which
runs each only while it answers, Slack pushes the app's messages to an
address instead, by its Events API (`lnk-slack hook`).

### Its two secrets come with the push

[`lnk agent connect slack`](../../../../../../../../../modules/agents/agent/docs/agents/README.md#connect-your-channels) keeps neither the app's signing secret nor a
configuration token, which can edit your Slack apps. Whatever pushes
the agent's messages gives them to `hook` in the app's settings
(`signing_secret`, `config_token`). The configuration token is used for
the switch and never kept.

### Without the signing secret

Without the signing secret, `hook` says where it is, Basic Information
and then App Credentials on the app's page, and switches nothing, so
the app stays in Socket Mode.

### Switching the app

With the app's configuration token, Link turns Socket Mode off and sets
the request URL itself, in the app's manifest (`apps.manifest.export`
and `apps.manifest.update`), and back when the pushes stop. Slack checks
the address as it's saved (`url_verification`), and Link answers.

### Two steps, by hand

Without a configuration token, or once Slack refuses it (one lasts 12
hours), Link gives you two steps on the app's page: Socket Mode off,
then Event Subscriptions' Request URL.

### What's read

Slack signs each push with the app's signing secret. Link drops one
whose signature is wrong, or whose timestamp is more than 5 minutes
from now, and reads the rest as the bridge does: your direct messages,
your mentions of the app, and members' mentions in the channels you
share. A push Slack sends again has the same event id, so it's answered
once. [Security](../security.md) has how a push is checked, and how the
signing secret rolls.

### Answers

Answers are posted with the bot token, as at home: Slack takes no
answer in a push's response. When Slack refuses the bot token (revoked,
or the app uninstalled), answering, or `lnk-slack send`, says so as its
own failure, apart from any other. So does an answer Slack refuses, its
conversation gone or archived, or the app no longer in it.

### Where the secrets stay

A signing secret and configuration token stay outside the sandbox, with
the tokens: a harness that speaks Slack is never handed them. A
connection made before keeps them, until it's connected again.
