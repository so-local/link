# Share a Conversation

Use `lnk agent share` to let people in a selected Slack channel, Telegram group, or Discord channel talk to your agent. Connect the app first. Sharing grants those people the agent's existing tools and files, so choose an agent with the access you intend to share.

## You and Your Shared Chats

Your agent answers your direct messages, and your mentions of the agent in a
group, connection, or server. The agent answers no one else, and nothing in a
group that doesn't mention it, unless you share that chat with it
(below). Add the bot to the group yourself. On Slack, `/invite` the bot to
the connection.

### A Conversation Each

Each chat is a conversation of its own, apart from your direct
messages. So is each Slack thread and each topic of a Telegram
forum. What's said in one stays out of the rest.

## Share It with a Chat

`share` prints a one-time code. Within 10 minutes, mention your bot with the code in a Slack
connection, a Telegram group, or a Discord connection (`@company 4K7P2QXM`).
Whoever is in that chat can then talk to your
agent there, mentioning the agent as you do.

```sh
lnk agent share add slack
```

### Who's Speaking

Your agent knows who's speaking, by name on the connection. Only
you can share, because the agent reads the code from your messages alone. Share on the agent's
machine before moving the agent to a cloud.

### What Sharing Gives Them

Sharing hands the chat your agent as it is, so these hold for everyone
in the chat:

- **Everyone can make your agent do all it can.** Sharing narrows nothing. The agent's
  files, tools, and memory are theirs to use through the agent. For a
  team, [make a second agent](../security.md#a-shared-chat) with only the team's files, and its own bot.
- **Some things stay yours.** Their direct messages to the bot go unanswered,
  and only you share, unshare, and approve what the agent asks.
- **You pay.** Their messages are turns of your agent, paid as yours
  are.
- **Not everywhere.** Signal can't be shared, and the agent needs
  a harness that can tell who's speaking (Link Harness).

### See and End a Share

`lnk agent share list` lists the chats your agent is shared with. [`lnk agent share remove`](../security.md#a-shared-chat) ends one, by its ID or name. [Your agent's specification](../../../../../contract/identity/docs/config/specs/access.md)
keeps the shares, as `[access] message`.

```sh
lnk agent share list
lnk agent share remove general --connection slack
```
