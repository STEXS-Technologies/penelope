"""Verify worker cleanup containment and recompute claim latency evidence."""
from pathlib import Path
import gzip,hashlib,itertools,json,re,subprocess,sys,tempfile
root=Path(__file__).resolve().parent
manifest=json.loads((root/'manifest.json').read_text())
for name,digest in manifest['sha256'].items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name

def read(name):
 p=root/name
 return p.read_text() if p.exists() else gzip.decompress(p.with_name(p.name+'.gz').read_bytes()).decode()
for compiler in ['stable','1.85.0']:
 for repeat in range(10):
  log=read(f'repeat-{compiler}-{repeat:02}.log');assert 'test result: ok. 11 passed; 0 failed' in log
  cases=re.findall(r'nested=(true|false) close_admission=(true|false) register_waker=(true|false) committed=2 dropped_once=true queued_completed=true',log)
  assert len(cases)==8 and set(cases)==set(itertools.product(['true','false'],repeat=3))
 for n in [29,51,42]:assert f'test result: ok. {n} passed; 0 failed' in read(f'final-{compiler}-sqlite.log')
plain=read('baseline-plain.log');assert 'left: Err(TimedOut)' in plain and 'right: Ok(2)' in plain and 'test result: FAILED' in plain
nested=read('baseline-nested.log');assert 'SIGABRT' in nested and 'thread result panicked on drop' in nested
for name in ['workspace.log','msrv.log','clippy.log']:
 log=read(name);assert 'error:' not in log and 'test result: FAILED' not in log
for n in [29,51,42]:assert f'test result: ok. {n} passed; 0 failed' in read('msrv.log')
assert 'Code graph updated' in read('graphify.log')
ci=json.loads(read('previous-ci.json'));assert ci['headSha']==manifest['baseline_commit'] and ci['conclusion']=='success' and ci['status']=='completed' and all(job['conclusion']=='success' for job in ci['jobs'])
hashes=json.loads(read('hashes.json'))
for name,digest in hashes.items():
 if name.endswith('.rs'):assert hashlib.sha256(read(name).encode()).hexdigest()==digest
assert 'let _job_result = contain_panic(|| job(&mut connection));' in read('candidate-worker.rs')
assert 'let _job_result = contain_panic(|| job(&mut connection));' not in read('baseline-worker.rs')
runs=[json.loads(s) for s in read('runs.jsonl').splitlines()];assert len(runs)==96
assert [(r['pair'],r['variant'],r['kind']) for r in runs]==[(pair,v,k) for pair in range(24) for v in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']) for k in ['empty','batch']]
for r in runs:
 expected={(False,0),(True,0)} if r['kind']=='empty' else set(itertools.product([False,True],[1,16]))
 assert len(r['rows'])==len(expected) and {(x['timers'],x.get('batch',0)) for x in r['rows']}==expected
 for row in r['rows']:assert row['iterations']==(2000 if r['kind']=='empty' else 400) and row['elapsed_ns']>0
with tempfile.TemporaryDirectory() as tmp:
 p=Path(tmp);(p/'runs.jsonl').write_text(read('runs.jsonl'));(p/'analyze.py').write_text(read('analyze.py'))
 subprocess.run([sys.executable,str(p/'analyze.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((p/'analysis.json').read_text())==json.loads(read('analysis.json'))
assert len(json.loads(read('analysis.json')))==6
print('Verified timeout/abort baseline controls, 20 worker-suite processes, 160 canceled-result scenarios, final qualification, 96 measured processes, six recomputed latency statistics and archive hashes.')
