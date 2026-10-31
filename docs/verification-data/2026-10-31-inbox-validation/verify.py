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
 for count in [33,57,42]:assert f'test result: ok. {count} passed; 0 failed' in read(name)
for name in ['baseline-tests.log','candidate-tests.log']:assert 'test result: ok. 57 passed; 0 failed' in read(name)
assert 'Code graph updated' in read('graph.log')
source=read('candidate-process.rs');baseline=read('baseline-process.rs')
body=source.split('fn accept_input(')[1].split('pub(crate) fn commit_process',1)[0]
assert 'prepare_cached' in body and 'column_bytes' in body and 'copy_column' not in body
assert 'if stored == bytes.as_slice()' in body and 'if Digest::of(stored).0.as_slice() != checksum' in body and 'decode(stored, checksum)?' in body
control=read('control-checksum.log');assert 'consumed=false case=checksum reopened=false' in control and 'left: Ok(' in control and 'test result: FAILED. 0 passed; 1 failed' in control
assert 'test result: ok. 2 passed; 0 failed' in read('restored-test.log')
for compiler in ['stable','1.85.0']:
 for repeat in range(10):
  log=read(f'repeat-{compiler}-{repeat:02}.log');assert 'inbox_validation::duplicate_admission_preserves_encodings_conflicts_and_corruption_rejection ... ok' in log and 'inbox_validation::stored_record_limit_bounds_noncanonical_duplicate_encodings ... ok' in log and 'test result: ok. 2 passed; 0 failed' in log
metadata=json.loads(read('timing-metadata.json'));hashes=json.loads(read('binary-hashes.json'))
assert metadata['campaigns']==2 and metadata['blocks_per_campaign']==12 and metadata['iterations_per_profile']==5000 and metadata['warmup_per_profile']==64
for label in ['baseline','candidate']:assert metadata['binary_sha256'][label]==hashes[label+'-inbox_accept']
assert baseline!=source
with tempfile.TemporaryDirectory() as tmp:
 p=Path(tmp);(p/'summarize.py').write_text(read('summarize.py'))
 for label in ['baseline','candidate']:
  for repeat in range(4):(p/f'{label}-{repeat}.jsonl').write_text(read(f'{label}-{repeat}.jsonl'))
 subprocess.run([sys.executable,str(p/'summarize.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((p/'summary.json').read_text())==json.loads(read('summary.json'))
with tempfile.TemporaryDirectory() as tmp:
 p=Path(tmp);(p/'timing_summary.py').write_text(read('timing_summary.py'))
 for campaign in range(2):(p/f'timing-{campaign}.jsonl').write_text(read(f'timing-{campaign}.jsonl'))
 subprocess.run([sys.executable,str(p/'timing_summary.py')],check=True,stdout=subprocess.DEVNULL)
 assert json.loads((p/'timing-summary.json').read_text())==json.loads(read('timing-summary.json'))
print('Verified source/archive hashes, all 512 allocation samples, 192 timing rows/96 processes and bootstrap summaries, checksum-bypass control, 20 repeated regression processes and stable/MSRV qualification.')
