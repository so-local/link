# Decisions

Application settings are shared by core and modules without depending on
the module protocol.

## Folder and output settings

Folder settings and output setup share one package because the
saved application settings contain `link-format`'s output settings.
Putting those settings in `link-files` would make the file and format
libraries depend on each other. `link-settings` sits above both and
exports its own operations; callers import either underlying library
directly for its types and helpers.
