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
 for count in [33,54,42]:assert f'test result: ok. {count} passed; 0 failed' in read(name)
for name in ['baseline-tests.log','candidate-tests.log']:assert 'test result: ok. 54 passed; 0 failed' in read(name)
assert 'Code graph updated' in read('graph.log')
source=read('candidate-leases.rs');baseline=read('baseline-leases.rs')
for symbol in ['outbox_lease','timer_lease']:
 body=source.split('fn '+symbol+'(')[1].split('\n}\n',1)[0]
 assert 'prepare_cached' in body and 'column_bytes' in body and 'copy_column' not in body
 assert 'if stored != encode(lease, limit)?' in body
assert baseline!=source
with tempfile.TemporaryDirectory() as tmp:
 p=Path(tmp);(p/'summarize.py').write_text(read('summarize.py'))
 for label in ['baseline','candidate']:
  for repeat in range(4):(p/f'{label}-{repeat}.jsonl').write_text(read(f'{label}-{repeat}.jsonl'))
 subprocess.run([sys.executable,str(p/'summarize.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((p/'summary.json').read_text())==json.loads(read('summary.json'))
print('Verified source/archive hashes, all 768 allocation samples, baseline/candidate process tests and stable/MSRV qualification. No latency claim.')
