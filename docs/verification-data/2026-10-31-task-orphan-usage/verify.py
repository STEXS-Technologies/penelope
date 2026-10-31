"""Verify quota-fix evidence and recompute the rejected experiment."""
from pathlib import Path
import gzip,hashlib,itertools,json,re,subprocess,sys,tempfile
root=Path(__file__).resolve().parent
manifest=json.loads((root/'manifest.json').read_text())
for name,digest in manifest['sha256'].items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name

def read(folder,name):
 p=folder/name
 return p.read_text() if p.exists() else gzip.decompress(p.with_name(p.name+'.gz').read_bytes()).decode()

def quota_cases(log):
 pattern=r'reused_input=(true|false) cold=(true|false) bound=(\d+) retained=(\d+) rejection_atomic=(true|false) exact_usage=true repaired_reopened=true'
 cases=re.findall(pattern,log);assert len(cases)==12
 assert {(reuse,cold,int(bound)-int(retained)) for reuse,cold,bound,retained,_ in cases}==set(itertools.product(['true','false'],['true','false'],[-1,0,1]))
 assert sum(rejected=='true' for *_,rejected in cases)==4
 for reuse,cold,bound,retained,rejected in cases:assert (rejected=='true')==(int(bound)<int(retained))
for compiler in ['stable','1.85.0']:
 for repeat in range(10):
  for suite,n in [('orphans',1),('cache',3),('memo',1)]:
   log=read(root,f'repeat-{compiler}-{repeat:02}-{suite}.log');assert f'test result: ok. {n} passed; 0 failed' in log
   if suite=='orphans':quota_cases(log)
   if suite=='cache':assert log.count('accepted=false unchanged=true')==48
   if suite=='memo':assert log.count('four_rejections_unchanged=true repaired_reopened=true')==9
 for n in [27,51,40]:assert f'test result: ok. {n} passed; 0 failed' in read(root,f'final-{compiler}-sqlite.log')
control=read(root,'baseline-control.log');assert 'left: Ok(TaskRecord' in control and 'right: Err(Exhausted)' in control and 'test result: FAILED' in control
for name in ['workspace.log','msrv.log','clippy.log']:
 log=read(root,name);assert 'error:' not in log and 'test result: FAILED' not in log
for n in [27,51,40]:assert f'test result: ok. {n} passed; 0 failed' in read(root,'msrv.log')
assert 'Code graph updated' in read(root,'graphify.log')
fixed=read(root,'fixed-tasks.rs');base=read(root,'baseline-tasks.rs')
assert 'let (mut projection, retained_used)' in fixed and 'let used = retained_used' in fixed
assert '.map_or(0, |(_, _, used)| *used)' in base
folder=root/'rejected-aggregate';runs=[json.loads(s) for s in read(folder,'runs.jsonl').splitlines()];assert len(runs)==480
configs=[('commits',e,False,False) for e in [False,True]]+[(k,False,a,p) for k in ['reconstruction','allocations'] for p,a in itertools.product([False,True],repeat=2)]
expected=[(pair,v,*config) for pair in range(24) for v in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']) for config in configs]
assert [(r['pair'],r['variant'],r['kind'],r['external'],r['alternating'],r['reformatted']) for r in runs]==expected
for r in runs:
 field='size' if r['kind']=='commits' else 'history';keys=([0,32,4096] if r['external'] else [0,32,4096,1048576]) if field=='size' else [2,16,128]
 assert sorted(set(x[field] for x in r['rows']))==keys
 assert len(r['rows'])==(48 if r['kind']=='allocations' else len(keys))
 for x in r['rows']:
  if r['kind']!='allocations':assert x['iterations']==(128 if r['kind']=='commits' else 250) and x['elapsed_ns']>0
  else:assert (x['allocations'],x['reallocations'],x['requested_bytes'])==((4,0,376) if r['variant']=='baseline' else (3,0,360))
with tempfile.TemporaryDirectory() as tmp:
 p=Path(tmp);(p/'runs.jsonl').write_text(read(folder,'runs.jsonl'));(p/'analyze.py').write_text(read(folder,'analyze.py'))
 subprocess.run([sys.executable,str(p/'analyze.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((p/'analysis.json').read_text())==json.loads(read(folder,'analysis.json'))
analysis=json.loads(read(folder,'analysis.json'));assert len(analysis['latency'])==19 and len(analysis['allocations'])==24
hashes=json.loads(read(folder,'hashes.json'))
for name,digest in hashes.items():
 if name.endswith(('.rs','.toml')):assert hashlib.sha256(read(folder,name).encode()).hexdigest()==digest
assert read(root,'baseline-tasks.rs')==read(folder,'baseline-tasks.rs')
assert hashes['baseline-Cargo.toml']==hashes['candidate-Cargo.toml']
print('Verified baseline rejection control, 240 quota fixtures, 60 regression processes, final SQLite qualification, 480 rejected-trial processes, 19 recomputed statistics, 9,216 allocation samples and archive hashes.')
