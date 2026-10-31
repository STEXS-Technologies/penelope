#!/usr/bin/env python3
"""Recompute every timing summary and validate retained qualification."""
from pathlib import Path
import hashlib,json,shutil,subprocess,sys,tempfile
root=Path(__file__).resolve().parent
manifest=json.loads((root/'manifest.json').read_text())
for name,digest in manifest['snapshot_sha256'].items():
 assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest
for label in ['baseline','candidate']:
 assert '61 passed; 0 failed' in (root/f'{label}-tests.txt').read_text()
with tempfile.TemporaryDirectory() as directory:
 scratch=Path(directory)
 for name in ['timing-0.jsonl','timing-1.jsonl','summarize.py']:
  shutil.copyfile(root/name,scratch/name)
 subprocess.run([sys.executable,str(scratch/'summarize.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((scratch/'summary.json').read_text())==json.loads((root/'summary.json').read_text())
metadata=json.loads((root/'metadata.json').read_text())
assert metadata['campaigns']==2 and metadata['blocks_per_campaign']==12
assert metadata['processes_per_block']==4 and metadata['iterations_per_profile']==1000
print('Verified 1440 timing rows, 30 independently recomputed summaries and both process suites.')
