import os
import sys
import types

class Clock:
    elapsed = 0
    def monotonic(self):
        return self.elapsed
time = Clock()

async def to_thread(call, *args, **kwargs):
    return call(*args, **kwargs)
async def sleep(seconds):
    time.elapsed += seconds
def run(coroutine):
    try:
        coroutine.send(None)
    except StopIteration as ended:
        return ended.value
    raise AssertionError('unexpected suspension in the owned discovery fixture')
asyncio = types.SimpleNamespace(to_thread=to_thread, sleep=sleep, run=run)

runtime = types.ModuleType('hermes_cli.gateway_runtime')
runtime.DEFAULT_ENSURE_TIMEOUT = 30
calls = []
states = []
def discover(home, **kwargs):
    calls.append(kwargs)
    return types.SimpleNamespace(state=states.pop(0))
runtime.discover_gateway_endpoint = discover
sys.modules['hermes_cli.gateway_runtime'] = runtime
ensured = []
def ensure_gateway_runtime(home, **kwargs):
    ensured.append(kwargs)
    return types.SimpleNamespace(state='native')

async def connect():
    home = object()
# MANAGED_BODY
    return result

os.environ['LNK_HERMES_MANAGED_GATEWAY'] = '1'
for sequence in (['starting', 'starting', 'ready'], ['absent'], ['inaccessible']):
    states[:] = sequence
    calls.clear()
    assert asyncio.run(connect()).state == sequence[-1]
    assert len(calls) == len(sequence)
    assert not ensured
runtime.DEFAULT_ENSURE_TIMEOUT = 0
states[:] = ['starting']
assert asyncio.run(connect()).state == 'starting'
del os.environ['LNK_HERMES_MANAGED_GATEWAY']
assert asyncio.run(connect()).state == 'native'
assert ensured == [{'idle_exit': True}]
