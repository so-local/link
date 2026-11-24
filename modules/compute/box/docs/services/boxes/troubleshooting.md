# Troubleshooting

What to do when a [box](README.md) won't make, start, show or save, symptom by
symptom. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk box list
```

## Making a box

What a `lnk box create` can say before the box is up.

### "Link on AWS needs its CLI" or "Link on Google Cloud needs its CLI"

[`lnk cloud connect`](../../../../../cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account) in a terminal installs it. On Linux, AWS's installer
needs `unzip`.

### "Couldn't ask a question this needs answered"

The command asks before it costs money or deletes something. Run it in a
terminal, or pass `--yes`.

### "nothing there fits"

Loosen the range it names. `lnk box size list <cloud>` lists what fits,
best first, making nothing.

### The cloud refuses a GPU or spot box

Ask it to raise your account's quota, in its console. Both clouds give a
new account no GPUs until you ask.

### "Made ... but couldn't set it up"

Its machine was made, and its setup didn't finish. `lnk box ssh <name>`
to look, or remove it and start again.

## Reaching a box

What to do when a box won't answer, or SSH refuses it. [Reach a Box](ssh.md#how-ssh-fails)
explains how each failure arises.

### "Couldn't tell which box you mean"

You have more than one box, so the command needs its name: `lnk box ssh
<name>`.

### "... is stopped"

`lnk box start <name>`. A spot box AWS took back starts again on its own
once AWS has room.

### "Couldn't find OpenSSH's ssh"

Boxes need OpenSSH's `ssh` and `ssh-keygen` on this computer, which a
Mac has already. Install OpenSSH with your system's package manager.

### "... which has no address yet"

The box was made, and its cloud hasn't said its address yet, or it was
last seen starting. `lnk box list --refresh` asks its cloud again.

### "Couldn't reach the box at ..., which didn't take SSH"

Its SSH server didn't answer within 5 minutes: the box may still be
booting, or your network blocks port 22. Try again in a minute, or set
`LNK_BOX_SSH=tunnel` for the [cloud's tunnel](ssh.md#through-the-clouds-tunnel).

### "REMOTE HOST IDENTIFICATION HAS CHANGED"

`ssh` refused the box's host key, which isn't the one Local Link pinned.
Don't accept it: check in your cloud's console that the machine at that
address is your box, and `lnk box list --refresh` learns your boxes'
addresses again, as [Changed Host Keys](ssh.md#changed-host-keys) explains.

## Finding your boxes

What to do when `lnk box list` misses a box, or lists one wrong.

### A box listed with no id

An interrupted `create` left it with no machine in the cloud: `lnk box
delete <name>`.

### A box made elsewhere doesn't show

`lnk box list --all-regions`, then check [`lnk cloud list`](../../../../../cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account) : the same cloud account and the same Link account.

### A box made before tags doesn't show

Run `lnk box list` on the computer that made it, with the box running,
which tags it.

## Images

What to do when an image won't save, or a box can't find one.

### "Couldn't list images"

Your cloud adapter, `lnk-aws` for one, doesn't answer the image commands.
[`lnk upgrade`](../../../../../../docs/more/modules/guide.md#upgrade) updates it.

### "Couldn't save ..., which has agents on it"

Save a box before an agent comes, or move the agents off it first: an
image would carry them to every box made from it.

### "Couldn't save ..., which holds secrets an image would carry"

Saving found secrets [where they're usually left](image/images.md#what-saving-refuses). Remove what it lists
from the box, then save again.

### "... stopped at the stage that failed"

Fix the command it names. `sh -x` on your script shows each command, and
the stages before it ran.

### "Couldn't find a ready image with the hash ..."

None saved with that hash yet, or still saving: `lnk box image list
--cloud <account> --hash <hash>`.
