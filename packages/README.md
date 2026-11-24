# Packages

Packages holds the shared libraries that Local Link's programs link. They cover clients, secrets, specifications, files, versions, printing, installation identity, services, testing, components, and sandbox bridges.

<!-- docs-tree:start -->

## Docs

### Pages

- [Shared Libraries](docs/more/libraries/README.md): Choose the library that supplies the behavior your caller needs
  - [Files and Secrets](docs/more/libraries/files/README.md): Files supplies filesystem and location primitives
  - [Questions and Effects](docs/more/libraries/interactions/README.md): Interactions coordinates questions, answers, and their effects through its shared contract
  - [Runtime and Communication](docs/more/libraries/runtime/README.md): Runtime provides an asynchronous runtime to synchronous entry points
  - [Types and Boundaries](docs/more/libraries/types/README.md): Build an interface from typed declarations before adding callers

### Packages

- [Actions](actions/README.md): `link-actions` reads every module's declarations strictly, resolves and checks them, and keeps them as one catalog
- [API Client](api/README.md): `link-api` is Local Link's typed client for the account service's API, with one function per route, built on `link-fetch`
- [Approvals](approvals/README.md): Approvals holds the pure rules that bind an approval to one reviewed effect, decide it, and let it be claimed once
- [Archive](archive/README.md): `link-archive` carries a thing's state as one `.tar.gz` file
- [Artifact](artifact/README.md): Artifact downloads pinned files, verifies each one, and keeps them in a shared cache of bounded size
- [Components](component/README.md): `link-component` describes a component in typed form
- [Controls](controls/README.md): Controls holds the pure rules that admit input, manage the life of work, and apply selection and delegation holds
- [Digest](digest/README.md): `link-digest` computes SHA-256 digests, which identify content and verify downloads
- [Fetch](fetch/README.md): `link-fetch` is an HTTP client with explicit transport policy, bounded responses, and consistent errors
- [Files](files/README.md): `link-files` keeps the files Local Link stores on this machine
- [Format](format/README.md): `link-format` renders everything `lnk` prints, as typed blocks that one formatter draws to a terminal, to a pipe, and as JSON
- [Interactions](interactions/README.md): Interactions coordinates questions, approvals, and work controls, and keeps them in one protected store
- [Keychain](keychain/README.md): `link-keychain` gives programs shared access to the macOS Keychain, and it stores secrets in private files on other machines
- [Machine](machine/README.md): `link-machine` knows this installation, the machine it runs on, and whose it is
- [Policy](policy/README.md): `link-policy` decides who may do what
- [Principal](principal/README.md): `link-principal` names who acts, or may act, as one type, `user:<id>` or `service:<id>`
- [Runtime](runtime/README.md): Runtime gives synchronous callers one reusable Tokio runtime
- [Secretive](secretive/README.md): `link-secretive` holds every secret Local Link keeps, in one of three types by where it may go: opaque, exposed only by name, or sealed to one recipient
- [Service Contract](service/README.md): `link-service` holds the shared background-service API, with native and explicitly selected Linux guest implementations
- [Settings](settings/README.md): `link-settings` holds Local Link's folder layout and application settings, so commands share them without depending on modules
- [Signer](signer/README.md): A signer and its verifier, Ed25519 signatures, the compact JWS every token Link mints is, and the short signed statement a principal trades for a token
- [Specifications](spec/README.md): `link-spec` reads specifications strictly, as typed TOML, and writes the paths in them so a specification means the same on any machine
- [Stable IDs](id/README.md): `link-id` holds stable identities, written `<kind>:<id>`, as one generic type that every kind of ID uses
- [Testing Kit](test-kit/README.md): `link-test-kit` holds the temp folders, programs, processes, and releases that tests need, written once so no test makes its own
- [Token](token/README.md): Link's one access token, a short signed JWT saying who acts and from which kind of client, never what's allowed, and the long-lived values traded for one
- [Types](types/README.md): `link-types` holds the shapes every module shares, such as an OS, a processor, a box's user, and a duration, with the report and MCP answers an action may give
- [Untrusted](untrusted/README.md): Untrusted puts one marker around every piece of third-party text that a tool hands a model
- [Version](version/README.md): `link-version` is a version of `lnk`, ordered as semantic versions are
- [Workplace](workplace/README.md): `link-workplace` says where work goes, as one type

<!-- docs-tree:end -->
