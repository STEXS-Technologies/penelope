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
 for count in [33,60,42]:assert f'test result: ok. {count} passed; 0 failed' in read(name)
for name in ['baseline-tests.log','candidate-tests.log']:assert 'test result: ok. 60 passed; 0 failed' in read(name)
assert 'Code graph updated' in read('graph.log')
source=read('candidate-inbox.rs');baseline=read('baseline-inbox.rs')
assert 'prepare_cached' in source and 'column_bytes' in source and 'copy_column' not in source
assert 'Vec::new()' in source and 'inputs.reserve_exact(usize::from(limit.get()))' in source
for label,case in [('checksum','checksum'),('bound','payload-size')]:
 control=read(f'control-{label}.log');assert f'case={case} reopened=false' in control and 'left: Ok(' in control and 'test result: FAILED. 0 passed; 1 failed' in control
assert 'test result: ok. 60 passed; 0 failed' in read('restored-process.log')
for compiler in ['stable','1.85.0']:
 for repeat in range(10):
  log=read(f'repeat-{compiler}-{repeat:02}.log')
  for name in ['discovery_is_bounded_scoped_repeatable_and_only_atomic_commit_consumes_inputs','discovery_rejects_corrupt_misindexed_and_oversized_inputs_without_consuming_them','warmed_discovery_tracks_admission_and_consumption_on_other_workers']:
   assert f'pending_inbox::{name} ... ok' in log
  assert 'test result: ok. 3 passed; 0 failed' in log
metadata=json.loads(read('timing-metadata.json'));hashes=json.loads(read('binary-hashes.json'))
assert metadata['campaigns']==2 and metadata['blocks_per_campaign']==12 and metadata['iterations_per_profile']==5000 and metadata['warmup_per_profile']==64 and metadata['pending_sizes']==[0,1,16]
for label in ['baseline','candidate']:assert metadata['binary_sha256'][label]==hashes[label+'-pending_inputs']
ci=json.loads(read('previous-ci.json'));assert ci['headSha']==manifest['baseline_commit'] and ci['status']=='completed' and ci['conclusion']=='success' and all(job['conclusion']=='success' for job in ci['jobs'])
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
print('Verified source/archive hashes, all 768 allocation samples, 288 timing rows/96 processes and bootstrap summaries, both checksum/bound controls, 20 repeated regression processes and stable/MSRV qualification.')
