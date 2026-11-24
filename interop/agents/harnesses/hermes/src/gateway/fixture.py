#!/usr/bin/env python3
"""Owned gateway stand-in: block stop signals before readiness, then wait atomically."""
import json
import os
from pathlib import Path
import signal
import sys

root = Path(os.environ['HERMES_HOME'])

stopping = {signal.SIGINT, signal.SIGTERM}
signal.pthread_sigmask(signal.SIG_BLOCK, stopping)
mode = os.environ.get('LNK_TEST_GATEWAY_MODE', 'ready')
(root / 'started').write_text(json.dumps({'pid': os.getpid()}))
if mode == 'fail':
    sys.exit(7)
if mode in ('ready', 'foreign'):
    pid = int(os.environ['LNK_TEST_OTHER_PID']) if mode == 'foreign' else os.getpid()
    (root / 'gateway_state.json').write_text(json.dumps({'pid': pid, 'gateway_state': 'running'}))
signal.sigwait(stopping)
(root / 'closed').write_text('native cleanup completed')
