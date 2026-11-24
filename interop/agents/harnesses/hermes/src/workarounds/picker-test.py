"""Owned registry/picker fixture: hidden metadata reads share the ACP policy."""
import concurrent.futures
import sys
import types

agent = types.ModuleType("agent")
registry = types.ModuleType("agent.models_dev")
agent.models_dev = registry
sys.modules["agent"] = agent
sys.modules["agent.models_dev"] = registry
exec(__SCOPE__, registry.__dict__)
registry.__dict__.update(_models_dev_cache={"cached": "native label"},
                         _models_dev_cache_time=0, _models_dev_retry_after=0)
fetch = "def fetch_models_dev(force_refresh=False, *, allow_network=True):\n" + __FETCH__ + "        return _models_dev_cache\n    return {'network': True}\n"
exec(fetch, registry.__dict__)

def read_hidden_metadata():
    # Metadata helpers default to live reads even when inventory is nonblocking.
    return registry.fetch_models_dev(allow_network=True)

namespace = {"read_hidden_metadata": read_hidden_metadata}
exec("from __future__ import annotations\n" + __PICKER__ +
     "    return read_hidden_metadata()\n", namespace)
picker = namespace["build_model_state"]
assert picker("known", "anthropic", "") == {"cached": "native label"}
registry._models_dev_cache = {}
assert picker("unknown", "anthropic", "") == {}
assert registry.fetch_models_dev(force_refresh=True) == {"network": True}
assert read_hidden_metadata() == {"network": True}

@namespace["_link_cached_model_picker"]
def fails():
    assert read_hidden_metadata() == {}
    raise ValueError("picker failed")

try:
    fails()
except ValueError:
    pass
else:
    raise AssertionError("picker failure was hidden")
assert read_hidden_metadata() == {"network": True}

@namespace["_link_cached_model_picker"]
def concurrent_reads():
    assert read_hidden_metadata() == {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
        assert pool.submit(read_hidden_metadata).result() == {"network": True}
    assert read_hidden_metadata() == {}

concurrent_reads()
assert registry._link_cached_picker.get() is False
