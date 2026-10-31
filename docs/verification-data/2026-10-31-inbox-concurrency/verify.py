"""Verify archived concurrent admission/commit qualification and controls."""
from pathlib import Path
import gzip,hashlib,json,subprocess,sys,tempfile
root=Path(__file__).resolve().parent
manifest=json.loads((root/'manifest.json').read_text())
for name,digest in manifest['sha256'].items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
def read(name):
 p=root/name
 return p.read_text() if p.exists() else gzip.decompress(p.with_name(p.name+'.gz').read_bytes()).decode()
for name,digest in json.loads(read('source-hashes.json')).items():assert hashlib.sha256(read(name).encode()).hexdigest()==digest,name
for name in ['workspace.log','msrv.log','clippy.log','restored-process.log','final-process-msrv.log']:
 log=read(name);assert 'error:' not in log and 'test result: FAILED' not in log,name
for name in ['workspace.log','msrv.log']:
 for n in [33,59,42]:assert f'test result: ok. {n} passed; 0 failed' in read(name)
for name in ['restored-process.log','final-process-msrv.log']:assert 'test result: ok. 59 passed; 0 failed' in read(name)
assert 'Code graph updated' in read('graph.log')
for label,case in [('consumption','consumed-input-at-next-frontier'),('source','reused-source-at-next-frontier')]:
 log=read(f'control-{label}.log');assert f'case={case} admitted=2 accepted=2' in log and 'test result: FAILED. 0 passed; 1 failed' in log
source=read('inbox_concurrency.rs');assert 'BEGIN IMMEDIATE' in source and 'future.as_mut().poll(&mut context).is_pending()' in source and 'ready.recv_timeout' in source
assert 'reference.results == results && reference.state == state' in source and 'exact_state=true reopened_retry=true' in source
ci=json.loads(read('previous-ci.json'));assert ci['headSha']==manifest['baseline_commit'] and ci['status']=='completed' and ci['conclusion']=='success' and all(job['conclusion']=='success' for job in ci['jobs'])
with tempfile.TemporaryDirectory() as tmp:
 p=Path(tmp);(p/'summarize.py').write_text(read('summarize.py'))
 for compiler in ['stable','1.85.0']:
  for repeat in range(10):(p/f'repeat-{compiler}-{repeat:02}.log').write_text(read(f'repeat-{compiler}-{repeat:02}.log'))
 subprocess.run([sys.executable,str(p/'summarize.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((p/'summary.json').read_text())==json.loads(read('summary.json'))
print('Verified source/archive hashes, both intentional gated-race failures, 20 repeated processes/160 races, exact serial states and reopened retries, final stable/MSRV qualification and baseline CI.')
