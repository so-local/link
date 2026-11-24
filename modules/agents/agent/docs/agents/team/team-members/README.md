# Team Members from Other Accounts

A [team](../README.md) on a cloud that hosts teams across accounts can
take an agent another account owns. `lnk agent team-member add` invites
the agent, and the agent's owner accepts with `join`.

## Invite an Agent from Another Account

On a cloud that hosts teams across accounts, both owners agree:

```sh
lnk agent team-member add design --cloud my-cloud --member acct-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/k3m4n5p6q7r8 --as reviewer --read . --write notes
lnk agent team join design --account acct-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa --cloud my-cloud --member reviewer
lnk agent team leave design --account acct-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa --cloud my-cloud --member reviewer
lnk agent team remove design --cloud my-cloud --member reviewer
```

## Join a Team

The invitation names the other account's permanent agent ID. The invited
agent's owner runs `join` using the agent's own hosted name or ID. Until
then, the agent receives neither messages nor the folder.

## Leave a Team

Either owner can end membership. The invited agent's owner uses `leave`, and the team's owner uses
`remove`. The hosting cloud refreshes active bindings when membership
changes. `lnk agent team list` includes hosted teams. Append `@<account>`
when more than one account has that team name.

## Set Folder Bounds

The team's owner chooses the invited agent's folder bounds. The invited agent's owner's
consent remains in force when the team owner later changes the file,
including those bounds. Removing and reinviting the agent requires fresh consent.

## Know Who Reads What

Whoever can talk to a member can cause the member to read the files the member can reach.
An outside agent's owner can read what the agent read in its conversations.
