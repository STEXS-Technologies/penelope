"""Verify archived bytes, raw campaign shape, tests, and recomputed statistics."""
from pathlib import Path
import gzip,hashlib,itertools,json,subprocess,sys,tempfile
root=Path(__file__).resolve().parent
manifest=json.loads((root/'manifest.json').read_text())
for name,digest in manifest['sha256'].items():
 assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name

def read(folder,name):
 p=folder/name
 return gzip.decompress(p.with_name(p.name+'.gz').read_bytes()).decode() if not p.exists() else p.read_text()

def primary(folder):
 runs=[json.loads(s) for s in read(folder,'runs.jsonl').splitlines()];assert len(runs)==480
 configs=[('commits',e,False,False) for e in [False,True]]+[(k,False,a,p) for k in ['reconstruction','allocations'] for p,a in itertools.product([False,True],repeat=2)]
 expected=[(pair,v,*config) for pair in range(24) for v in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']) for config in configs]
 assert [(r['pair'],r['variant'],r['kind'],r['external'],r['alternating'],r['reformatted']) for r in runs]==expected
 for r in runs:
  field='size' if r['kind']=='commits' else 'history'; keys=([0,32,4096] if r['external'] else [0,32,4096,1048576]) if field=='size' else [2,16,128]
  assert sorted(set(x[field] for x in r['rows']))==keys
  assert len(r['rows'])==(48 if r['kind']=='allocations' else len(keys))
  for x in r['rows']:
   if r['kind']!='allocations':assert x['iterations']==(128 if r['kind']=='commits' else 250) and x['elapsed_ns']>0
   else:assert x['reallocations']==0
 with tempfile.TemporaryDirectory() as tmp:
  p=Path(tmp);(p/'runs.jsonl').write_text(read(folder,'runs.jsonl'));(p/'analyze.py').write_text(read(folder,'analyze.py'))
  subprocess.run([sys.executable,str(p/'analyze.py')],check=True,stdout=subprocess.DEVNULL)
  assert json.loads((p/'analysis.json').read_text())==json.loads(read(folder,'analysis.json'))
 stats=json.loads(read(folder,'analysis.json'));assert len(stats['latency'])==19 and len(stats['allocations'])==24
 if folder==root:
  assert all(x['metrics']==[1,0,96] for x in stats['allocations'])
 return len(runs)
count=sum(primary(p) for p in [root,root/'rejected-raw',root/'eager-scope'])
rows=[json.loads(s) for s in read(root,'confirm-runs.jsonl').splitlines()];assert len(rows)==144
orders=list(itertools.permutations(['pre_guard','baseline','candidate']))
assert [(r['block'],r['external_commit'],r['variant']) for r in rows]==[(block,e,v) for block in range(24) for e in ([False,True] if block%2==0 else [True,False]) for v in orders[block%6]]
for r in rows:
 assert [x['size'] for x in r['rows']]==([0,32,4096] if r['external_commit'] else [0,32,4096,1048576])
 assert all(x['iterations']==128 and x['elapsed_ns']>0 for x in r['rows'])
with tempfile.TemporaryDirectory() as tmp:
 p=Path(tmp);(p/'confirm-runs.jsonl').write_text(read(root,'confirm-runs.jsonl'));(p/'analyze-confirm.py').write_text(read(root,'analyze-confirm.py'))
 subprocess.run([sys.executable,str(p/'analyze-confirm.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((p/'confirm-analysis.json').read_text())==json.loads(read(root,'confirm-analysis.json'))
for compiler in ['stable','1.85.0']:
 for repeat in range(10):
  prefix=f'repeat-{compiler}-{repeat:02}-'
  for suite,n in [('differential',3),('columns',1),('cache',3),('memo',1)]:
   log=read(root,prefix+suite+'.log');assert f'test result: ok. {n} passed; 0 failed' in log
   if suite=='differential':assert 'directed=189 checksum_controls=189 mutations=10000' in log and 'scope_byte_boundaries=256' in log
   if suite=='cache':assert log.count('accepted=false unchanged=true')==48
   if suite=='memo':assert log.count('four_rejections_unchanged=true repaired_reopened=true')==9
negative=read(root,'negative-control.log');assert 'test result: FAILED' in negative and 'left: Ok(' in negative and 'right: Err(Corrupt)' in negative
for name in ['workspace.log','msrv.log','clippy.log']:
 log=read(root,name);assert 'error:' not in log and 'test result: FAILED' not in log
for n in [27,51,38]:assert f'test result: ok. {n} passed; 0 failed' in read(root,'msrv.log')
hashes=json.loads(read(root,'hashes.json'))
for name,digest in hashes.items():
 if name.endswith(('.rs','.toml')):assert hashlib.sha256(read(root,name).encode()).hexdigest()==digest
assert hashes['baseline-Cargo.toml']==hashes['candidate-Cargo.toml']
provenance=json.loads(read(root,'confirm-provenance.json'))
assert provenance['sha256']['baseline']==hashes['baseline-task_blob_commits']
assert provenance['sha256']['candidate']==hashes['candidate-task_blob_commits']
assert json.loads(read(root,'rebuild-control.json'))['identical']
print(f'Verified {count+len(rows)} measured processes, 78 recomputed latency statistics, 27,648 allocation samples, 80 final regression processes and archive hashes.')
