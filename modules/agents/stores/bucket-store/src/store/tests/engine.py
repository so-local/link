#!/usr/bin/env python3
"""Local rclone object protocol fixture; reads each complete put before commit."""
import json
import os
from pathlib import Path
import sys
import tempfile

args = sys.argv[1:]
while args and args[0].startswith("--"):
    flag = args.pop(0)
    if flag == "--config":
        args.pop(0)
command = args.pop(0)
path = Path(args[-1])
if command == "cat":
    if not path.is_file():
        sys.exit(3)
    count = int(args[args.index("--count") + 1])
    with path.open("rb") as source:
        sys.stdout.buffer.write(source.read(count))
elif command == "rcat":
    body = sys.stdin.buffer.read()
    if len(body) != int(args[args.index("--size") + 1]):
        sys.exit(1)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as target:
        target.write(body)
        aside = target.name
    os.replace(aside, path)
elif command == "deletefile":
    path.unlink(missing_ok=True)
elif command == "lsjson":
    files = sorted(path.rglob("*")) if path.is_dir() else []
    json.dump([
        {"Path": str(file.relative_to(path)), "Size": file.stat().st_size}
        for file in files if file.is_file()
    ], sys.stdout)
else:
    sys.exit(2)
