//! Exact Python source fragments of the pinned release, never executed here.

pub(super) const DEPENDENCY_HOME: (&str, &str) = (
    "    override = get_hermes_home_override()\n",
    "    root = Path(__file__).resolve().parent.parent\n    payload = _payload_manifest(root)\n    if payload is not None:\n        return payload[1]\n    override = get_hermes_home_override()\n",
);
pub(super) const STORE: (&str, &str) = (
    r#"    if honor_runtime_override:
        override = os.environ.get("HERMES_RUNTIME_DIR")
        if override:
            return Path(override).resolve()
    root = Path(project_root).resolve()
    payload = _payload_manifest(root)
    store = payload[0].get("store") if payload else None
    if store:
        return _payload_path(payload[1], store, "store")
"#,
    r#"    root = Path(project_root).resolve()
    payload = _payload_manifest(root)
    store = payload[0].get("store") if payload else None
    if store:
        return _payload_path(payload[1], store, "store")
    if honor_runtime_override:
        override = os.environ.get("HERMES_RUNTIME_DIR")
        if override:
            return Path(override).resolve()
"#,
);
pub(super) const ACTIVATION: (&str, &str) = (
    "    if state.is_dir():\n        from hermes_cli.runtime_state import runtime_lock, recover_publication, lease_generation\n",
    "    if payload_venv(project_root) is None and state.is_dir():\n        from hermes_cli.runtime_state import runtime_lock, recover_publication, lease_generation\n",
);
pub(super) const SELECTION: (&str, &str) = (
    "def _recorded_venv(project_root: Path) -> Path | None:\n    path = runtime_facts_path(project_root)\n",
    "def _recorded_venv(project_root: Path) -> Path | None:\n    environment = payload_venv(project_root)\n    if environment is not None:\n        return environment\n    path = runtime_facts_path(project_root)\n",
);
pub(super) const WRITABLE_STORE: (&str, &str) = (
    r#"def writable_store_root() -> Path:
    if not (store_root().parent / "manifest.json").is_file():
        return store_root()
    from hermes_constants import get_default_hermes_root

    return get_default_hermes_root() / "tools"
"#,
    "def writable_store_root() -> Path:\n    return store_root()\n",
);
