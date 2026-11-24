# Decisions

Why Link Harness reads [images and documents](README.md) the way it does.

## Why are images and documents files beside the events?

An event's line is read at every turn and never changed, so a photo in
it would be read with every message after. Each is a file in `parts/`,
named by a random token, which the event names with its kind, media
type, name and size.

### Checked as it comes and as it's read

An image or PDF is checked by its first bytes, and text by being UTF-8,
as it comes and again as it's read: one changed since is left out,
saying so.

### Why the bridge downloads them

They come as OpenAI's content parts with `data:` URLs only, since the
harness, sandboxed and holding no platform's token, fetches nothing. A
connection's bridge holds the token, so it downloads each for a harness
whose contract says `parts`, checked against the connection contract's
types and limits (`link_connection_contract::MEDIA`, `PART_MAX`,
`PARTS_MAX`).
