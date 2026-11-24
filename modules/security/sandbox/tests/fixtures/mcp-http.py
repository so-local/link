# A stand-in MCP server over streamable HTTP, on a free port of this
# machine's loopback (written to argv[1]). It answers `initialize` with
# JSON and a session, and `tools/list` and `tools/call` as an event
# stream; a call's answer says the Authorization header it got.
import http.server, json, sys

class Server(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        method, auth = body.get("method"), self.headers.get("Authorization", "")
        if "id" not in body:
            self.send_response(202); self.end_headers(); return
        if method == "initialize":
            result = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                      "serverInfo": {"name": "stand-in", "version": "1"}}
            self.reply(body["id"], result, json_only=True); return
        if self.headers.get("Mcp-Session-Id") != "s-1":
            self.send_response(404); self.end_headers(); return
        if method == "tools/list":
            result = {"tools": [{"name": "echo", "inputSchema": {"type": "object"}}]}
        elif method == "tools/call":
            result = {"content": [{"type": "text", "text": "echoed with " + auth}]}
        else:
            result = {}
        self.reply(body["id"], result)

    def reply(self, id, result, json_only=False):
        data = json.dumps({"jsonrpc": "2.0", "id": id, "result": result}).encode()
        self.send_response(200)
        self.send_header("Mcp-Session-Id", "s-1")
        if json_only:
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers(); self.wfile.write(data); return
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        self.wfile.write(b"event: message\ndata: " + data + b"\n\n")

    def do_DELETE(self):
        self.send_response(200); self.end_headers()

    def log_message(self, *a): pass

server = http.server.HTTPServer(("127.0.0.1", 0), Server)
open(sys.argv[1] + ".tmp", "w").write(str(server.server_port))
import os; os.rename(sys.argv[1] + ".tmp", sys.argv[1])
server.serve_forever()
