# Decisions

Installation identity has one owner, independent of the modules that use it.

## Identity and Settings

The machine package owns the installation's names and IDs, and host facts. Agent restrictions
and secret-keeper settings belong to their modules, with separate
typed settings files. A module can change its settings without
expanding the identity contract used by unrelated programs.

## Serialized identity creation

A stable sidecar lock serializes first creation even when the document
is atomically replaced. Trusted-file checks precede reads and writes.
An existing document must contain a valid ID and name, with no other
fields, so a malformed identity cannot silently create a different one.
