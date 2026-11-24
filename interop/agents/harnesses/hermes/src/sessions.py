"""Hermes's sessions for Link's ASP, through its own SessionDB.

`export` prints every session, each message its owner or Hermes said;
`import` takes sessions on stdin, each into a session of ACP's, once,
and prints each one's new id by its old. An imported session is a plain
`acp` row, with no route or provenance, so Hermes's gateway adopts it as
the owner's own when ACP's `session/resume` names it: a row carrying
`origin_json`, as `import_foreign_history` writes, it only shows read-only,
and its resume answers `not_found`.
"""
import hashlib
import json
import sys
import uuid

from hermes_state import SessionDB


def message(raw):
    fields = ("role", "content", "tool_calls", "tool_call_id", "reasoning_content", "reasoning_details")
    result = {k: raw[k] for k in fields if raw.get(k) is not None and (k in ("role", "content") or raw[k])}
    if raw.get("is_summary"):
        result["is_summary"] = True
    if not result.get("reasoning_content") and raw.get("reasoning"):
        result["reasoning_content"] = raw["reasoning"]
    if isinstance(result.get("tool_calls"), str):
        result["tool_calls"] = json.loads(result["tool_calls"])
    result["raw"] = raw
    return result


def mint(session):
    """The id of `session`'s import: the same thread at the same state
    always the same, so a second import of it is the first's, and a later
    version of it a new snapshot. It has the shape of an id `hermes acp`
    mints (a version 4 UUID)."""
    portable = json.dumps(session["portable"], sort_keys=True, separators=(",", ":"))
    digest = hashlib.sha256((session["id"] + "\n" + portable).encode()).digest()
    return str(uuid.UUID(bytes=digest[:16], version=4))


def main():
    db = SessionDB()
    if sys.argv[1] == "export":
        sessions = []
        for session in db.export_all(include_compacted=True, include_inactive=True):
            messages = [message(m) for m in session.get("messages") or []]
            sessions.append({"id": session["id"], "title": session.get("title"), "messages": messages})
        json.dump(sessions, sys.stdout)
    elif sys.argv[1] == "import":
        taken = {}
        for session in json.load(sys.stdin):
            made = mint(session)
            title = session.get("title")
            held = db.get_session_by_title(title) if title else None
            if held and held["id"] != made:
                # Titles are unique in a profile; the id's tail keeps it readable.
                title = f"{title} ({made[-12:]})"
            result = db.import_sessions([{
                "id": made, "source": "acp", "title": title,
                "cwd": session["cwd"], "messages": session["messages"],
            }])
            if not result["ok"]:
                raise SystemExit(f"Hermes refused session {session['id']}: {result['errors']}")
            taken[session["id"]] = made
        json.dump(taken, sys.stdout)


main()
