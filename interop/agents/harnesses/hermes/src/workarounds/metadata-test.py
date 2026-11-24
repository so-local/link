for base in (
    'http://127.0.0.1:40781/_link/model',
    'http://localhost:40781/_link/model',
    'http://[::1]:40781/_link/model',
):
    assert _link_hosted_relay(base)
    assert _normalize_base_url(base) == base + '/v1'
    assert _normalize_base_url(base + '/v1/') == base + '/v1'
    assert _query_ollama_api_show('model', base) is None

for base in (
    'http://127.0.0.1:11434/v1',
    'http://127.0.0.1:40781/_link/here/11434/v1',
    'https://api.anthropic.com',
    'http://example.com/_link/model',
    'http://127.0.0.1.evil.example/_link/model',
    'http://owner@127.0.0.1/_link/model',
    'http://127.0.0.1/_link/model?route=other',
    'http://127.0.0.1/_link/model#other',
    'http://[invalid',
):
    assert not _link_hosted_relay(base), base
    assert _normalize_base_url(base) == base, base

calls = []
def _memo_local_probe(key, probe):
    calls.append(key)
    return 32768

def _strip_provider_prefix(model):
    return model

for base in ('http://127.0.0.1:11434/v1', 'https://ollama.example/v1'):
    assert _query_ollama_api_show('model', base) == 32768
    assert calls[-1] == ('ollama_show', 'model', base)
assert len(calls) == 2

# The test owns this module; no foreign harness code is imported.
import sys
import types
metadata = types.ModuleType('agent.model_metadata')
metadata._link_hosted_relay = _link_hosted_relay
sys.modules['agent.model_metadata'] = metadata
for base in ('http://127.0.0.1:40781/_link/model', 'https://api.anthropic.com',
             'https://gateway.example/anthropic/v1'):
    assert _is_anthropic_compatible_host(base), base
for base in ('http://127.0.0.1:11434/v1', 'https://openrouter.ai/api/v1',
             'http://example.com/_link/model', 'http://127.0.0.1.evil.example/_link/model',
             'http://127.0.0.1/_link/model?route=other'):
    assert not _is_anthropic_compatible_host(base), base
