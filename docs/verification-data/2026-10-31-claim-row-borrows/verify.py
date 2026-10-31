"""Verify claim row borrowing allocation, latency and qualification evidence."""
from pathlib import Path
import gzip,hashlib,itertools,json,subprocess,sys,tempfile
root=Path(__file__).resolve().parent
manifest=json.loads((root/'manifest.json').read_text())
for name,digest in manifest['sha256'].items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name

def read(name):
 p=root/name
 return p.read_text() if p.exists() else gzip.decompress(p.with_name(p.name+'.gz').read_bytes()).decode()
for compiler in ['stable','1.85.0']:
 for repeat in range(10):
  log=read(f'repeat-{compiler}-{repeat:02}.log');assert 'test result: ok. 13 passed; 0 failed' in log and log.count('test lease_contracts::')==13
for name in ['workspace.log','msrv.log','clippy.log','allocation-clippy.log']:
 log=read(name);assert 'error:' not in log and 'test result: FAILED' not in log
for name in ['workspace.log','msrv.log']:
 for n in [31,51,42]:assert f'test result: ok. {n} passed; 0 failed' in read(name)
assert 'Code graph updated' in read('graph.log')
ci=json.loads(read('previous-ci.json'));assert ci['headSha']==manifest['baseline_commit'] and ci['conclusion']=='success' and ci['status']=='completed' and all(job['conclusion']=='success' for job in ci['jobs'])
hashes=json.loads(read('hashes.json'))
for name,digest in hashes.items():
 if name.endswith('.rs'):assert hashlib.sha256(read(name).encode()).hexdigest()==digest
candidate=read('candidate-leases.rs');baseline=read('baseline-leases.rs')
assert candidate.count('let bytes = column_bytes(row,0,limits.max_record_bytes)')==2
assert baseline.count('let bytes = copy_column(row,0,limits.max_record_bytes)')==2
assert candidate.count('let checksum = column_bytes(row,1,32)')==2
assert 'column_bytes(row, index, limit).map(<[u8]>::to_vec)' in read('column-helper.rs')
assert 'allocation_probe::start' not in read('lease_batch.rs')
assert 'allocation_probe::start' in read('lease_batch_allocations.rs')
for prefix in ['', 'confirmation/']:
 assert 'tmpfs' in json.loads(read(prefix+'storage.json'))['filesystem']
 runs=[json.loads(s) for s in read(prefix+'runs.jsonl').splitlines()];assert len(runs)==96
 assert [(r['pair'],r['variant'],r['kind']) for r in runs]==[(pair,v,k) for pair in range(24) for v in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']) for k in ['empty','batch']]
 for r in runs:
  expected={(False,0),(True,0)} if r['kind']=='empty' else set(itertools.product([False,True],[1,16]))
  assert len(r['rows'])==len(expected) and {(x['timers'],x.get('batch',0)) for x in r['rows']}==expected
  for row in r['rows']:assert row['iterations']==(2000 if r['kind']=='empty' else 400) and row['elapsed_ns']>0
 with tempfile.TemporaryDirectory() as tmp:
  p=Path(tmp);(p/'runs.jsonl').write_text(read(prefix+'runs.jsonl'));(p/'analyze.py').write_text(read(prefix+'analyze.py'))
  subprocess.run([sys.executable,str(p/'analyze.py')],check=True,stdout=subprocess.DEVNULL)
  assert json.loads((p/'analysis.json').read_text())==json.loads(read(prefix+'analysis.json'))
 assert len(json.loads(read(prefix+'analysis.json')))==6
with tempfile.TemporaryDirectory() as tmp:
 p=Path(tmp);(p/'allocations.jsonl').write_text(read('allocations.jsonl'));(p/'analyze-allocations.py').write_text(read('analyze-allocations.py'))
 subprocess.run([sys.executable,str(p/'analyze-allocations.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((p/'allocation-analysis.json').read_text())==json.loads(read('allocation-analysis.json'))
assert len(json.loads(read('allocation-analysis.json')))==4
print('Verified source/archive hashes, 20 lease-contract processes, final qualification, 192 latency processes, 12 recomputed latency comparisons and all 512 allocation samples.')
