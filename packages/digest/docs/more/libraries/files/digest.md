# Digest

`link-digest` provides SHA-256 and hexadecimal encoding for content
identities, independently of the module protocol.

## Content identities

`sha256` returns a lowercase hexadecimal digest of bytes.
`sha256_bytes` returns its 32 bytes, and `sha256_of` streams from any
`Read` implementation without loading the whole input into memory.
`hex` encodes arbitrary bytes in the same lowercase representation.

## Typed content

`content_digest` names a value by the SHA-256 of its canonical JSON:
object keys sorted lexicographically, arrays kept in order, compact UTF-8.
A typed value and the same value carried as plain JSON therefore digest
alike, whatever their struct field order. `ContentId` holds such a digest as
lowercase hexadecimal and refuses anything else when read. Callers can name
captured inputs, images, and requests without depending on their display names.
