# Sandbox Log

Every call a sandbox makes past its walls goes through its proxy, on a
wire. The proxy writes a line for each call. The log is how you see what a
sandbox reached, and what it was refused.

## See Every Call on the Wire

Each line is a call on a sandbox's proxy: when (UTC), which sandbox, the
verb, where to, and what happened. The log never holds a request's path,
headers or body.

```sh
lnk sandbox log show
lnk sandbox log show --lines 200 --sandbox main-link
lnk sandbox log show --refused
lnk sandbox log show --json
```

## Sandbox Names and Verbs

A sandbox is named by what started it: the workload and the program
it runs, a box user's name, or `run-<pid>`. The verb says what kind of call it was, such as `connect`
for HTTPS, `tcp` for another port or `model` for a hosted model, and
[the wire's log](../../../../../proxy/docs/security/sandbox/more/proxy/wire.md#the-wires-log) lists them all.

## Read What Happened to a Call

Each line ends with one result. A call is `allowed`, then `ended` with the
bytes each way. Otherwise the call is `refused`, or `failed` when it was
allowed but nothing answered. An upstream socket refused for being
another user's says so after `failed`.
