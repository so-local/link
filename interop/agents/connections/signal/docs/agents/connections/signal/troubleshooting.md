# Troubleshooting

What to do when your agent's [Signal number](README.md) doesn't link or doesn't
answer.

## "Signal isn't linked on ..."

The agent's number isn't linked on this machine. Connect again in a
terminal and scan the code from the agent's phone.

```sh
lnk agent connect signal
```

## `connect signal` says the phone is another number

The QR code was scanned from a phone of another number, such as yours.
Scan it from the agent's own phone, the number you gave as the agent's.

## A Mac's `signal-cli` can't reach Signal

`signal-cli`'s Java can't send the secret the sandbox's proxy needs on a
Mac, so Signal's servers answer through the proxy with a `407`. Give
the agent `[network] mode = "host"` in its sandbox file, or run it on
Linux.
