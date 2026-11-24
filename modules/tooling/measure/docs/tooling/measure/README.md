# Measure

> Measure What Each Agent Costs, as It Happens

Measure shows what each of your agents costs you as it happens, including
memory, CPU, tokens and spend, and the same for this machine. It records
numbers only, never a message, prompt, file, tool argument or answer. You
need macOS or Linux and the `measure` module, which `lnk up` installs.
Measure is free. You can also [send what the watcher sees](export/README.md)
to an observability tool of yours.

<!-- quickstart -->

## See Current Resource Use

See this machine’s agents, processes and current resource use. The [measure guide](guide.md) lists the measurements you can see and the historical views.

```sh
lnk measure
```

## Start the Watcher

The watcher looks at the machine every 10 seconds. The guide shows how to [keep watching](guide.md#keep-watching) over time.

```sh
lnk measure start
```

## Show Your Measure Settings

Show your measurement settings and their defaults. The guide covers [recording intervals and retention](guide.md#keep-watching).

```sh
lnk measure get
```

## Export Measurements

Send measurements to your OpenTelemetry collector, using your collector’s address. The guide explains the [protocol and the recorded data](guide.md#send-them-to-your-observability-tools).

```sh
lnk measure export set --url http://localhost:4318
```

## More

Measure keeps history and can send it out, and other programs can supply
or read its numbers.

### Find History and Fix Export

[Files](files.md) says where Local Link keeps measurements
and export settings. [Security](security.md) lists what Measure reads and
sends. When history is missing or an export fails,
[Troubleshooting](troubleshooting.md) names the cause.

### Read the Reasons and the Protocol

[Decisions](decisions.md) gives the reasons for the measurement and
retention choices. The [Measure Protocol](../../../contract/docs/tooling/measure/more/protocol.md)
defines the numeric observations other programs can supply and read.

<!-- docs-tree:start -->

## Docs

### Pages

- [Measure Guide](guide.md): Inspect current resource use, retain a history, and send numeric measurements to your observability tools
- [Files](files.md): The files `lnk measure` keeps, and what each one holds
- [Measure Export](export/README.md): The watcher keeps what it sees on this machine

### More

- [Decisions](decisions.md): Why `lnk measure` works the way it does
- [Security](security.md): What `lnk measure` reads, keeps and sends, and what it never does
- [Troubleshooting](troubleshooting.md): What to do when [`lnk measure`](README.md) shows no history, can't watch, or sends nothing

<!-- docs-tree:end -->
