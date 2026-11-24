"""Owned fragments: schema reads never dial; actual image routing still can."""
from typing import Optional, Any
from types import ModuleType, SimpleNamespace
import sys
from concurrent.futures import ThreadPoolExecutor
from threading import Barrier
from contextvars import ContextVar

__LOOKUP__

image = ModuleType('agent.image_routing')
image._link_schema_only = _link_schema_only
image._link_schema_call = _link_schema_call
sys.modules['agent.image_routing'] = image
models = ModuleType('agent.models_dev')
sys.modules['agent.models_dev'] = models
calls = []
def capabilities(provider, model, *, allow_network):
    calls.append(allow_network)
    return SimpleNamespace(supports_vision=True) if allow_network or model == 'cached' else None
models.get_model_capabilities = capabilities
auxiliary = ModuleType('agent.auxiliary_client')
probe = ContextVar('owned_aux_probe', default=False)
auxiliary._aux_probe_active = probe.get
sys.modules['agent.auxiliary_client'] = auxiliary

scope = compile(__SCOPED__, '<native-schema-call>', 'exec')
def schema(callback):
    state = dict(entry=SimpleNamespace(dynamic_schema_overrides=callback))
    exec(scope, state)
    return state['overrides']

assert schema(lambda: _probe_models_dev('anthropic', 'cached', None)) is True
assert schema(lambda: _probe_models_dev('anthropic', 'unknown', None)) is None
assert _probe_models_dev('anthropic', 'unknown', None) is True
assert calls == [False, False, True]
token = probe.set(True)
assert _probe_models_dev('anthropic', 'unknown', None) is None
assert calls[-1] is False
probe.reset(token)
assert _probe_models_dev('anthropic', 'unknown', None) is True
assert calls[-1] is True
try:
    schema(lambda: (_ for _ in ()).throw(ValueError('fixture failure')))
except ValueError:
    pass
else:
    raise AssertionError('schema failure was hidden')
assert not _link_schema_only.get()

# One schema lookup must never suppress another thread's real image routing.
barrier = Barrier(2)
def held():
    def lookup():
        barrier.wait()
        barrier.wait()
        return _link_schema_only.get()
    return schema(lookup)
with ThreadPoolExecutor(max_workers=1) as pool:
    future = pool.submit(held)
    barrier.wait()
    assert _probe_models_dev('anthropic', 'unknown', None) is True
    barrier.wait()
    assert future.result() is True

# Availability is unchanged, including a native-only vision model.
for auxiliary in (False, True):
    for native in (False, True):
        checked = []
        def auxiliary_check():
            checked.append('auxiliary')
            return auxiliary
        def native_check():
            checked.append('native')
            return native
        gate = dict(check_video_requirements=auxiliary_check,
                    _should_use_native_vision_fast_path=native_check)
        exec('def available():\n' + __GATE__, gate)
        assert gate['available']() == (auxiliary or native)
        assert checked == (['auxiliary'] if auxiliary else ['auxiliary', 'native'])
