# Troubleshooting

An environment failure names the file line, stage or service involved.
Use the matching symptom below to check the input or read the failing
command's output before starting it again.

## A file reports a line number

Fix that line of the environment file. `lnk env check --spec env.toml`
uses the same strict reader as setup and startup, so it reproduces
schema and missing-input refusals without running the stages.

## A secret has no yes

A message such as `NPM_TOKEN: environment app at ~/app/env.toml hasn't
been given it, and no one is here to say yes` means the run couldn't ask
you. Say yes ahead with the command it names, then run it again. Copies
and moved files ask again. Image builds and services receive no secret
values.

```sh
lnk env approval add NPM_TOKEN --spec env.toml
```

## The box has no environment record

If Link says the box was not made from an environment on this computer,
pass the original environment file with `--env`. The automatic lookup
uses the record saved when this computer created the box.

## An image build reaches its deadline

Give the build a longer `--timeout`, or find the slow stage in its
output. A failed or timed-out build removes its temporary box, just as
a successful build does after saving its image.

## A service is not ready after 60 seconds

Check the service and its `ready` command. Connect with
[`lnk box ssh <box>`](../../../../box/docs/services/boxes/ssh.md#open-a-shell-on-a-box) and read `~/.lnk/safe/envs/logs/`, or increase the
readiness timeout when the service needs more time to start.

## Setup fails at a stage

The named stage's command failed. Read the output printed above the
failure, correct that command or its inputs, then run setup again with
the environment file so successful reusable stages can be retained.
