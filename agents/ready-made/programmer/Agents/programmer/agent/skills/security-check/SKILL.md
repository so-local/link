---
name: security-check
description: Check a change for injection, secrets, unsafe deserialization, path traversal and other common vulnerabilities.
---

# Security Check

Run this on any change that handles input from outside the program: users,
files, network, environment, other processes or a database. Read the diff
and ask each question.

- **Injection.** Is any untrusted text put into a SQL query, a shell
  command, an HTML page, a template, a regex, a log line or a URL? Use
  parameterized queries, process argument arrays without shell evaluation,
  and context-appropriate output encoding and escaping. Follow the matching
  language skill and existing safe APIs.
- **Secrets.** No keys, tokens, passwords or private URLs in code, tests,
  fixtures, logs, errors or commits. Read them from the environment or a
  secret store. Check `git diff` and `git status` for `.env`, `*.pem`,
  credentials files. If you find a real secret, stop and report it; do not
  copy it anywhere.
- **Path traversal.** A path built from input must be joined to a base
  and checked after resolving to stay inside it. Refuse `..`,
  absolute paths and symlinks that leave the base. Beware zip/tar
  extraction.
- **Unsafe deserialization.** Never evaluate untrusted serialized data or
  load it through a decoder that can construct executable objects. Use a
  data-only format, parse into typed structures, and validate.
- **Validation.** Check type, length, range and format at the boundary;
  refuse the unexpected rather than guess. Bound sizes, depth and counts
  to avoid memory and CPU exhaustion; set timeouts on network calls.
- **Authentication and authorization.** Is every new endpoint or command
  checked for who may call it? Compare secrets in constant time. Do not
  roll your own crypto; use the platform's library with safe defaults, and
  random values from a cryptographic source.
- **Memory safety.** Bounds, lifetimes, integer
  overflow, format strings, use after free. Prefer safe types and run
  sanitizers.
- **Errors and logs.** No secrets or personal data in them, and no
  internal detail returned to an outside caller.
- **TLS and requests.** Never disable certificate verification.
- **Dependencies.** New ones go through the dependencies skill.

Fix what you find in your own change. Report anything pre-existing that you
notice but did not change, with its file and line.
