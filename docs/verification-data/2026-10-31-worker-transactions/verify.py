#!/usr/bin/env python3
"""Validate durable transaction scenario and qualification logs."""
import itertools,json
from pathlib import Path
root=Path(__file__).resolve().parent
runs=json.loads((root/'repeats.json').read_text());assert len(runs)==20
expected={f"committed={commit} canceled={cancel} closed={closed}: durable history and queue verified" for commit,cancel,closed in itertools.product(['false','true'],repeat=3)}
for toolchain in ['stable','1.85.0']:
 selected=[r for r in runs if r['toolchain']==toolchain]
 assert sorted(r['repeat'] for r in selected)==list(range(10))
 for run in selected:
  text=(root/run['log']).read_text()
  assert run['exit']==0 and '1 passed; 0 failed' in text
  actual=[line for line in text.splitlines() if 'durable history and queue verified' in line]
  assert len(actual)==8 and set(actual)==expected
for name in ['stable.txt','msrv.txt']:
 assert '34 passed; 0 failed' in (root/name).read_text()
control=(root/'control.txt').read_text()
assert 'left: Err(TimedOut)' in control and 'right: Err(Unavailable)' in control
assert '0 passed; 1 failed' in control
assert 'Finished' in (root/'clippy.txt').read_text()
print('Verified 160 gated transaction scenarios, both 34-test unit suites, and unresolved-response control.')
