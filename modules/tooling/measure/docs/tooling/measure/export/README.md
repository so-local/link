# Measure Export

The watcher keeps what it sees on this machine. It can also send each look
to an OTLP (OpenTelemetry Protocol) endpoint you name, such as an
OpenTelemetry collector, Grafana, or Honeycomb.

## Set Where Looks Go

`lnk measure export set` does one of three things, and its flag says which.
`--url` sends every look to an endpoint, `--off` stops sending, and `--job`
sends one retained job's profile once. `lnk measure get` says where looks
go now. The guide explains [what is sent and where it may go](../guide.md#send-them-to-your-observability-tools).

```sh
lnk measure export set --url http://localhost:4318
lnk measure export set --off
```
