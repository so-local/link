#!/bin/sh
# A stdio MCP server for tests: its tools `echo` (says it reads) and
# `delete_all` (says it deletes); a call answers with its HOME and
# $TOKEN. Request members may arrive in any order. Each start is a line
# of $HOME/starts.
echo started >> "$HOME/starts"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":[[:space:]]*\([^,}]*\).*/\1/p')
  case "$line" in
    *'"error"'*|*'"result"'*) ;;
    *'"method":"initialize"'*)
      # A request of the client first, which Link answers itself.
      printf '{"jsonrpc":"2.0","id":"s1","method":"roots/list"}\n'
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}}\n' "$id" ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"echo","description":"Echoes","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}},{"name":"delete_all","description":"Deletes everything","inputSchema":{"type":"object"},"annotations":{"destructiveHint":true}}]}}\n' "$id" ;;
    *'"method":"tools/call"'*)
      printf '%s\n' "$line" >> "$HOME/calls"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"home=%s token=%s"}]}}\n' "$id" "$HOME" "$TOKEN" ;;
    *'"method":"notifications/'*) ;;
    *) [ -n "$id" ] && printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id" ;;
  esac
done
