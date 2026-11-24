---
name: go
description: Build, test, lint and write idiomatic Go in a module with go.mod.
---

# Go

## Detect

`go.mod` names the module and the Go version; `go.work` joins modules;
`vendor/` means vendored dependencies (`-mod=vendor`). `Makefile` or CI
may wrap commands; `.golangci.yml` configures lint. Check `go version`.

## Commands

```sh
go build ./...
go test ./...                       # one: go test ./pkg -run TestName -v
go test -race -count=1 ./...        # races, no cached results
go vet ./...
gofmt -l .                          # list unformatted; gofmt -w <files> to fix
golangci-lint run                   # if configured and installed
go mod tidy                         # after dependency changes
govulncheck ./...                   # if installed
```

## Idioms

- Return errors, wrap with context: `fmt.Errorf("read config: %w", err)`;
  test with `errors.Is`/`errors.As`. Check every error; no ignored `_`.
- Accept interfaces, return structs; define interfaces at the consumer;
  keep them small. Zero values should be useful.
- `context.Context` is the first parameter of blocking or request-scoped
  functions; honor cancellation. Always `defer` closes and cancel funcs.
- Goroutines need an owner and a way to stop; share by communicating,
  guard shared state with a mutex; use `sync.WaitGroup`/`errgroup`.
- Table-driven tests with `t.Run`, `t.Helper()`, `t.TempDir()`,
  `t.Parallel()` only if safe. Doc comments start with the name.
- Package names are short and lowercase; exported names are capitalized.

## Pitfalls

Loop variable capture (fixed in Go 1.22 only if go.mod says so); nil map
writes; nil interface holding a nil pointer; `defer` in loops; unbuffered
channel deadlocks; `time.After` leaks; modifying a slice shared through
append. Commit `go.mod` and `go.sum` together.
