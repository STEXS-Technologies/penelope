"""Check source/archive hashes, allocation samples and final qualification logs."""
from pathlib import Path
import gzip,hashlib,json,subprocess,sys,tempfile
root=Path(__file__).resolve().parent
manifest=json.loads((root/'manifest.json').read_text())
for name,digest in manifest['sha256'].items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
def read(name):
 p=root/name
 return p.read_text() if p.exists() else gzip.decompress(p.with_name(p.name+'.gz').read_bytes()).decode()
for name,digest in json.loads(read('source-hashes.json')).items():assert hashlib.sha256(read(name).encode()).hexdigest()==digest,name
for name in ['workspace.log','msrv.log','baseline-tests.log','candidate-tests.log','clippy.log']:
 value=read(name);assert 'error:' not in value and 'test result: FAILED' not in value,name
for name in ['workspace.log','msrv.log']:
 for count in [33,55,42]:assert f'test result: ok. {count} passed; 0 failed' in read(name)
for name in ['baseline-tests.log','candidate-tests.log']:assert 'test result: ok. 55 passed; 0 failed' in read(name)
assert 'Code graph updated' in read('graph.log')
source=read('candidate-process.rs');baseline=read('baseline-process.rs')
body=source.split('fn verify_committed_batch(')[1]
assert 'prepare_cached' in body and 'column_bytes' in body and 'copy_column' not in body
assert 'decode::<ProcessOutcome>(bytes, checksum)?' in body
control=read('control-checksum.log');assert 'outcome/checksum' in control and 'left: Ok(' in control and 'test result: FAILED. 0 passed; 1 failed' in control
assert 'test result: ok. 1 passed; 0 failed' in read('restored-test.log')
for compiler in ['stable','1.85.0']:
 for repeat in range(10):
  log=read(f'repeat-{compiler}-{repeat:02}.log');assert 'commit_integrity::receipt_reconstruction_rejects_missing_and_substituted_constituents ... ok' in log and 'test result: ok. 1 passed; 0 failed' in log
assert baseline!=source
with tempfile.TemporaryDirectory() as tmp:
 p=Path(tmp);(p/'summarize.py').write_text(read('summarize.py'))
 for label in ['baseline','candidate']:
  for repeat in range(4):(p/f'{label}-{repeat}.jsonl').write_text(read(f'{label}-{repeat}.jsonl'))
 subprocess.run([sys.executable,str(p/'summarize.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((p/'summary.json').read_text())==json.loads(read('summary.json'))
print('Verified source/archive hashes, all 512 allocation samples, outcome checksum-bypass control, 20 repeated regressions, baseline/candidate process tests and stable/MSRV qualification. No latency claim.')
