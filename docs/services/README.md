# Cloud

> Cloud Machines and Storage in Your Accounts

Connect your own clouds, then create boxes to run programs and buckets to
keep files. Each task below builds on the one before, from your first
account to your agent on a box.

<!-- quickstart -->

## Connect a Cloud

Connect your Amazon Web Services or Google Cloud account, then list the
accounts you connected. Have your cloud credentials ready. Connecting
creates nothing, and your provider bills you only for what you create.
You can connect [more than one account](../../modules/cloud/cloud/docs/services/cloud/README.md).

```sh
lnk cloud connect aws|gcp
lnk cloud list
```

## Create a Box

Create an Ubuntu machine in the cloud account you connected, with `lnk`
on it, signed in as you. You choose a [box's](../../modules/compute/box/docs/services/boxes/README.md)
size and image, and you remove the box when you finish. Your cloud bills
you for the box, and Local Link bills nothing.

```sh
lnk box create aws|gcp
```

## Open a Shell on a Box

Open a shell on a box you created and run commands there. SSH is one of
[several ways to reach a box](../../modules/compute/box/docs/services/boxes/ssh.md#open-a-shell-on-a-box).

```sh
lnk box ssh
```

## Set Up a Machine From a File

An [environment file](../../modules/compute/env/docs/services/environments/README.md)
says what a box needs before work starts. It names the OS, the stages
that set the box up, and the services the box runs. One file can make a
box, a job, or an agent's machine. Check each file before you make a
machine.

```sh
lnk env check --spec env.toml
```

## Back Up Your Folders

Create a bucket in your cloud account, then copy a folder to the bucket.
[Storage](../../modules/storage/storage/docs/services/storage/README.md)
verifies each copy and restores your files when you need them. Choose
the folders and storage accounts to back up, and verify the copies before
you remove the local files.

```sh
lnk storage create aws|gcp
lnk storage push photos/2019
```

## Run Your Agent in the Cloud

Your agent starts on this computer with [`lnk agent start`](../../modules/agents/agent/docs/agents/README.md#create-an-agent) and keeps
running in the background,
but it answers only while this computer is on. On a Linux server it
keeps running after you log out. Move your agent to a box in your cloud,
with its files, conversations, and memory, and bring it back to this
computer whenever you like.
[Move Your Agent](../../modules/agents/agent/docs/agents/move/README.md)
shows every option.

```sh
lnk agent move aws|gcp
lnk agent move here
```

## Create an Agent on a Box

Create a new agent straight on a box you made, rather than moving one
there. The agent runs on the box from its first start, and
[Create Another Agent](../../modules/agents/agent/docs/agents/start/create.md)
covers where else an agent can start.

```sh
lnk agent create work --on aws-1
```

## Grant Your Agent a Secret

A secret stays on your computer and goes only where you grant it, never
to a file just because the file names it. Keep a secret, then grant it
to your agent wherever the agent runs.
[Secrets](../../modules/security/secret/docs/config/secrets/README.md)
explains each grant.

```sh
lnk secret create NPM_TOKEN --internet-host registry.npmjs.org
lnk secret add NPM_TOKEN --to agent:main
```
