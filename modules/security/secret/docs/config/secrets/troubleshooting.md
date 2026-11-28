# Troubleshooting

What to do when [`lnk secret`](README.md) refuses a grant, leaves a copy behind, or
can't reach its keeper. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk secret list
```

## A secret is refused for an agent

Told `... is granted to main plain, and an agent is given a secret
sealed only`, create it again with `--internet-host <its host>`, then give it to the agent with `lnk secret add`. Told `main is on
aws-1: a secret is granted to it there`, run `lnk box ssh aws-1`, then
`lnk secret create` and `lnk secret add` there.

## A store of before IDs is refused

Told `it keeps secrets by their names, as an earlier lnk did`, your
store was written before secrets had IDs: remove it (`rm
~/.lnk/safe/secrets/secrets.toml`), add each secret again with `lnk
secret create`, and start your agents again for their own.

## An agent's keys are asked for again

Told `main's keys are kept with your secrets now ...`, give each again
as the message says, with [`lnk agent start`](../../../../../agents/agent/docs/agents/README.md#start-your-agent) or [`lnk agent connect`](../../../../../agents/agent/docs/agents/connections/connect/README.md#choose-an-app).

## A copy stays behind

Told `secret:k3m4n5p6q7r8 is left on aws-1: ...`, the box was stopped
or offline: start it with [`lnk box start aws-1`](../../../../../compute/box/docs/services/boxes/lifecycle/README.md#start-a-box), then run `lnk secret
delete secret:k3m4n5p6q7r8` again. Told `... on cloud-1: removed with
the agent there`, remove the agent there (`lnk agent delete`), or move
it away from there.

## The keeper doesn't answer

Told `your secrets' keeper isn't answering at
/run/lnk-secrets/keeper.sock`, its service is stopped: `sudo systemctl
start lnk-secrets`, and `journalctl -u lnk-secrets` says why it
stopped.

## The keeper is busy

The keeper answers a few calls of each user at once, and the rest
wait their turn. Told `your secrets' keeper ... closed the call
unanswered: it's busy with your other calls`, or that it `was busy with
your other calls for the whole wait`, programs of yours made more calls
at once than it would hold: run the command again once they've ended.

## Two stores of your secrets

Told `your secrets' keeper already keeps secrets of yours, and so does
...`, your own file came back beside the keeper's: create each of its
secrets again with `lnk secret create`, then remove the file.

## No systemd

Told `the keeper runs as a service of systemd, which this machine
doesn't run`, your secrets stay in your own file, mode 600, which any
program you run outside a sandbox can read.
