# Hermes's SessionDB standing in for `sessions.py`'s test: rows kept in
# a JSON file between runs, `import_sessions` taking each new id and
# skipping one it holds, as Hermes 0.23.0's hermes_state_portability.py
# does. The sessions asked for come from a file, on stdin.
import json
import os
import sys
import types

ROWS = os.environ["SESSIONS_TEST_ROWS"]
sys.stdin = open(os.environ["SESSIONS_TEST_ASKED"])


def load():
    try:
        with open(ROWS) as file:
            return json.load(file)
    except FileNotFoundError:
        return {}


class SessionDB:
    def get_session_by_title(self, title):
        return next((row for row in load().values() if row.get("title") == title), None)

    def import_sessions(self, sessions):
        rows = load()
        for session in sessions:
            if session["id"] not in rows:
                rows[session["id"]] = session
        with open(ROWS, "w") as file:
            json.dump(rows, file)
        return {"ok": True, "errors": []}

    def import_foreign_history(self, origin, messages, *, title, cwd, profile):
        rows = load()
        made = "foreign-" + str(len(rows))
        rows[made] = {"id": made, "source": origin["tool"], "title": title, "cwd": cwd,
                      "messages": messages, "origin_json": json.dumps({"imported_from": origin})}
        with open(ROWS, "w") as file:
            json.dump(rows, file)
        return {"session_id": made, "already_imported": False}


module = types.ModuleType("hermes_state")
module.SessionDB = SessionDB
sys.modules["hermes_state"] = module
