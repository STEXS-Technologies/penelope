#!/usr/bin/env python3
"""Check all retained authority histories, qualification and tombstone fault control."""
from pathlib import Path
from collections import Counter
import hashlib,json,re
root=Path(__file__).resolve().parent
manifest=json.loads((root/'manifest.json').read_text())
assert hashlib.sha256((root/'race-test.txt').read_bytes()).hexdigest()==manifest['test_snapshot_sha256']
runs=json.loads((root/'repeats.json').read_text());assert len(runs)==20
counts=Counter()
pattern=re.compile(r'^case=(\d+) round=(\d+) model=(reserve-first|fence-first|both) fenced_identity_preserved=true stopped=true delayed_start_rejected=true$',re.M)
for toolchain in ['stable','1.85.0']:
 selected=[r for r in runs if r['toolchain']==toolchain]
 assert sorted(r['repeat'] for r in selected)==list(range(10))
 for run in selected:
  text=(root/run['log']).read_text()
  assert run['exit']==0 and '1 passed; 0 failed' in text
  rows=pattern.findall(text);assert len(rows)==24
  assert {(int(c),int(r)) for c,r,_ in rows}=={(c,r) for c in range(3) for r in range(8)}
  for case,_,model in rows:
   assert (model=='both')==(case=='2')
   counts[f'{case}:{model}']+=1
assert dict(counts)==json.loads((root/'models.json').read_text())
for case in ['0','1']:
 assert counts[f'{case}:reserve-first']>0 and counts[f'{case}:fence-first']>0
for name in ['stable.txt','msrv.txt']:
 assert '44 passed; 0 failed' in (root/name).read_text()
control=(root/'control.txt').read_text()
assert 'left: Ok(Reserved)' in control and 'right: Err(Conflict)' in control and '0 passed; 1 failed' in control
restored=(root/'restored.txt').read_text()
assert '1 passed; 0 failed' in restored and len(pattern.findall(restored))==24
assert 'Finished' in (root/'clippy.txt').read_text()
assert all(v==0 for v in manifest['qualification_exits'].values())
print('Verified 480 contention histories, 960 competing requests, both serial models, 44-test suites and fault control.')
