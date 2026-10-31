#!/usr/bin/env python3
"""Verify retained WAL checkpoint scenarios and qualification logs."""
from pathlib import Path
import json
root=Path(__file__).resolve().parent
runs=json.loads((root/'repeats.json').read_text());assert len(runs)==20
marker='busy=true pinned_records=2 released_records=3 truncated=true history_unchanged=true reopened=true'
for toolchain in ['stable','1.85.0']:
 selected=[r for r in runs if r['toolchain']==toolchain]
 assert sorted(r['repeat'] for r in selected)==list(range(10))
 for run in selected:
  text=(root/run['log']).read_text()
  assert run['exit']==0 and '1 passed; 0 failed' in text and text.count(marker)==1
for name in ['stable.txt','msrv.txt']:
 assert '43 passed; 0 failed' in (root/name).read_text()
control=(root/'control.txt').read_text()
assert 'left: 0' in control and 'right: 1' in control and '0 passed; 1 failed' in control
assert 'Finished' in (root/'clippy.txt').read_text()
assert all(value==0 for value in json.loads((root/'manifest.json').read_text())['qualification_exits'].values())
print('Verified 20 WAL busy/truncation scenarios, both 43-test task suites and false-success control.')
