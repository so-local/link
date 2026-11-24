# Box Key

Local Link lets you into every box with one SSH (Secure Shell) key of its
own. It makes the key the first time a box needs it. Give the key's public
half to any machine that lets you in by a key, such as a server of your own.

## Show the Key

`lnk box key show` prints the key's public half as one OpenSSH line and
changes nothing. With `--secret-file`, it prints where the secret half is
kept, for `ssh -i`. It never prints the secret half itself.

```sh
lnk box key show
lnk box key show --secret-file
```
