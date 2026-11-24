---
name: python
description: Build, test, lint and write idiomatic Python, picking uv, poetry, pip or tox from the project's files.
---

# Python

## Detect

- `uv.lock` or `[tool.uv]`: use `uv` (`uv sync`, `uv run pytest`).
- `poetry.lock` or `[tool.poetry]`: `poetry install`, `poetry run ...`.
- `requirements*.txt`: a virtualenv and `pip`.
- `pyproject.toml` holds build backend and tool config (`[tool.pytest]`,
  `[tool.ruff]`, `[tool.mypy]`); `setup.py`/`setup.cfg` is older style;
  `tox.ini` or `noxfile.py` defines CI sessions. Check `python3 --version`
  against `requires-python`.

Never install into the system Python. Create or reuse a virtualenv:
`python3 -m venv .venv && . .venv/bin/activate`, and keep `.venv` ignored.

## Commands

```sh
pip install -e ".[dev]"            # or: uv sync / poetry install
pytest -q                          # one: pytest path/to/test.py::test_name -x
ruff check . && ruff format --check .   # or black/isort/flake8 if configured
mypy .  # or pyright, if the project uses a type checker
python3 -m build                   # packages
```

Use the repository's configured tools; do not add a new linter.

## Idioms

- Type hints on public functions; `dataclass` or pydantic for records;
  `pathlib.Path` over string paths; context managers (`with`) for files
  and locks; f-strings; comprehensions where clear.
- Raise specific exceptions, catch narrowly, never bare `except:`. Do not
  use mutable default arguments. Use `is None`, not `== None`.
- Use `logging`, not `print`, in libraries. Use `subprocess.run([...],
  check=True)` without `shell=True`. Prefer `yaml.safe_load`, never `pickle`
  on untrusted data.
- Tests with pytest: fixtures, `parametrize`, `tmp_path`, `monkeypatch`.

## Pitfalls

Import-time side effects; circular imports; relying on dict or set order
of unhashables; naive datetimes (use timezone-aware); unpinned
dependencies; tests that depend on the working directory; async code that
calls blocking functions.
