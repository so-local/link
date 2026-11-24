#!/bin/sh
# A stdio MCP server for tests that doesn't end when its input does: it
# answers initialize, writes its process ID to $HOME/stubborn.pid, and
# stays.
echo $$ > "$HOME/stubborn.pid"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":[[:space:]]*\([^,}]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"stubborn","version":"1"}}}\n' "$id" ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[]}}\n' "$id" ;;
    *'"method":"notifications/'*) ;;
    *) [ -n "$id" ] && printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id" ;;
  esac
done
while true; do sleep 1; done
