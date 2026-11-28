# Decisions

Why [bringing an existing agent under Link](README.md)
copies its state and rewrites paths in the forms that own them.

## Why copy a database through SQLite?

A live SQLite database may keep recent writes in its write-ahead log.
Stopping the harness's service and using SQLite's online backup gives
the copy those writes. Database headers identify these files before
text rewriting, and absolute paths inside them are rewritten through
SQL, so a database that happens to be valid UTF-8 is never edited as text.

## Why open an original read-only beside its log?

A log or shared memory beside an original database means a harness
holds it or stopped without closing, and its log may hold writes not
yet in the database. Opened read-only, those writes are copied but
never folded into the original.

## Why open an original able to write alone?

With nothing beside it, an original database is opened able to write,
though nothing is written. Opened read-only, SQLite would make a log
and shared memory beside it and couldn't take them away; able to
write, it removes them on closing, with nothing to fold in.

## Why rewrite the deepest paths first?

A harness's home can contain several agents and workspaces. Replacing
an outer path first would hide a more specific path from its own
mapping. Applying the deepest mappings first sends each copied file
to the agent and workspace it belongs to.

### Why leave portable evidence unchanged?

Native paths must follow the runtime to its new home. Portable source
records and sourced memory revisions are immutable evidence, so copying
preserves `.lnk/data/agents/context/` byte for byte. Rewriting an archived tool
result would change its meaning and break source correlation on return.

## Why does each profile become an agent?

An OpenClaw agent or Hermes profile has state of its own. Giving each
its own Link agent preserves that separation and lets the owner leave
one out before copying. The shared service is stopped once; a shared
secret is added to Link's secrets once and kept in the other copy's
settings where needed.
