# Sandbox Questions

Some calls ask you first before the sandbox lets them through. One kind
is a tool's call you haven't allowed, and another is a page to open in
your browser. Each is a question with a short code, answered once, yes
or no.

## Answer What a Sandbox Asks

A call that asks you first, such as a tool's or a page to open, shows a
dialog on this computer's screen. It also asks on the terminal the
sandbox started from, with a six-character code to type back. For a
sandbox that a background program started, it asks on your chats too.
Two minutes without an answer is no, and ten for such a program's. The
first answer counts. The sandbox's settings and the starting program's
specification [change the wait](../README.md#asking-you).

```sh
lnk sandbox question list
lnk sandbox answer k7m2pq yes|no
```

`lnk sandbox question list` shows each question in a table of its `ID`,
the `QUESTION` and the time it has `LEFT`, after which it counts as no.

## With No One to Ask

When no dialog, terminal, or chat can ask, such as for a program in the
background on a box, a tool's call is refused at once. The refusal names
the `lnk sandbox tool add <tool> --add <call>` that lets the call
through. With no desktop, a page to open is refused at once, because no
browser can open it. [A call that asks by one of its arguments](../../reach.md#asked-by-an-argument)
(`--ask-if`) asks even when `--add` names it, so its refusal names
what was added with the tool as letting it through (`--ask-if-hint`),
such as an owner's setting. Without one, the refusal names the argument
to give `false`.
