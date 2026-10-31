#!/usr/bin/env python3
"""Validate the race reproduction/fix, allocation samples and complete timing analysis."""
from pathlib import Path
import hashlib,json,shutil,subprocess,sys,tempfile
root=Path(__file__).resolve().parent
manifest=json.loads((root/'manifest.json').read_text())
for name,digest in manifest['snapshot_sha256'].items():
 assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest
old=(root/'baseline-regression.txt').read_text()
assert 'left: Err(Corrupt)' in old and 'right: Ok(4)' in old and '0 passed; 1 failed' in old
marker='retirement_committed_during_read=true pinned_blob=true later_conflict=true reopened=true'
assert marker in (root/'candidate-regression.txt').read_text()
runs=json.loads((root/'repeats.json').read_text());assert len(runs)==20
for toolchain in ['stable','1.85.0']:
 selected=[r for r in runs if r['toolchain']==toolchain]
 assert sorted(r['repeat'] for r in selected)==list(range(10))
 for run in selected:
  text=(root/run['log']).read_text()
  assert run['exit']==0 and '1 passed; 0 failed' in text and text.count(marker)==1
for label,expected in [('baseline',(2,0,160)),('candidate',(1,0,176))]:
 for repeat in range(4):
  rows=[json.loads(line) for line in (root/f'{label}-alloc-{repeat}.jsonl').read_text().splitlines()]
  assert len(rows)==128
  for size in [0,32,4096,1048576]:
   for corrupt in [False,True]:
    samples=[r for r in rows if r['size']==size and r['corrupt']==corrupt]
    assert sorted(r['sample'] for r in samples)==list(range(16))
    assert all(tuple(r[k] for k in ['allocations','reallocations','requested_bytes'])==expected for r in samples)
 assert '43 passed; 0 failed' in (root/f'{label}-tests.txt').read_text()
with tempfile.TemporaryDirectory() as directory:
 scratch=Path(directory)
 for name in ['timing-0.jsonl','timing-1.jsonl','summarize.py']: shutil.copyfile(root/name,scratch/name)
 subprocess.run([sys.executable,str(scratch/'summarize.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((scratch/'summary.json').read_text())==json.loads((root/'summary.json').read_text())
for name in ['workspace.txt','msrv.txt']:
 text=(root/name).read_text()
 for count in [35,61,43]: assert f'{count} passed; 0 failed' in text
 assert 'test result: FAILED' not in text
assert all(value==0 for value in manifest['qualification_exits'].values())
assert 'Finished' in (root/'clippy.txt').read_text()
print('Verified baseline race failure, 20 fixed interleavings, 1024 allocation samples, 768 timing rows, 16 summaries and full qualification.')
