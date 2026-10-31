#!/usr/bin/env python3
"""Check all allocation samples, recompute timing summaries and verify qualification."""
from pathlib import Path
import hashlib,json,shutil,subprocess,sys,tempfile
root=Path(__file__).resolve().parent
manifest=json.loads((root/'manifest.json').read_text())
for name,digest in manifest['snapshot_sha256'].items():
 assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest
for label,expected in [('baseline',(0,0,0)),('candidate',(1,0,80))]:
 for repeat in range(4):
  rows=[json.loads(line) for line in (root/f'{label}-alloc-{repeat}.jsonl').read_text().splitlines()]
  assert len(rows)==128
  for size in [0,32,4096,1048576]:
   for corrupt in [False,True]:
    samples=[r for r in rows if r['size']==size and r['corrupt']==corrupt]
    assert sorted(r['sample'] for r in samples)==list(range(16))
    assert all(tuple(r[k] for k in ['allocations','reallocations','requested_bytes'])==expected for r in samples)
 assert '42 passed; 0 failed' in (root/f'{label}-tests.txt').read_text()
with tempfile.TemporaryDirectory() as directory:
 scratch=Path(directory)
 for name in ['timing-0.jsonl','timing-1.jsonl','summarize.py']: shutil.copyfile(root/name,scratch/name)
 subprocess.run([sys.executable,str(scratch/'summarize.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((scratch/'summary.json').read_text())==json.loads((root/'summary.json').read_text())
for name in ['workspace.txt','msrv.txt']:
 text=(root/name).read_text()
 for count in [34,61,42]: assert f'{count} passed; 0 failed' in text
 assert 'test result: FAILED' not in text
assert all(value==0 for value in manifest['qualification_exits'].values())
assert 'Finished' in (root/'clippy.txt').read_text()
print('Verified 1024 allocation samples, 768 timing rows, 16 recomputed summaries and qualification.')
