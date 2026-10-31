"""Read-only verification of rejected and retained direct action key experiments."""
from pathlib import Path
import gzip
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
def digest(data):return hashlib.sha256(data).hexdigest()
def log(folder,name):return gzip.decompress((folder/(name+'.txt.gz')).read_bytes()).decode()
def main():
 actual={str(p.relative_to(HERE)):digest(p.read_bytes()) for p in HERE.rglob('*') if p.is_file() and p.name!='manifest.json'}
 assert actual==json.loads((HERE/'manifest.json').read_text())
 for name,expected in json.loads((HERE/'source-hashes.json').read_text()).items():assert digest((ROOT/name).read_bytes())==expected,name
 for label in ['initial','inlined']:
  folder=HERE/label;metadata=json.loads((folder/'metadata.json').read_text())
  for name,expected in metadata['common_sha256'].items():assert digest((ROOT/name).read_bytes())==expected,name
  for variant,files in metadata['harness_sha256'].items():
   for name,expected in files.items():
    data=(ROOT/name).read_bytes()
    if variant=='baseline':data=data.replace(b'.canonical_effect_key_bytes()',b'.effect_key().canonical_bytes()')
    assert digest(data)==expected,(label,variant,name)
  with tempfile.TemporaryDirectory() as temporary:
   scratch=Path(temporary)
   for name,expected in metadata['runtime_sources']['baseline'].items():
    data=subprocess.check_output(['git','show',metadata['parent']+':'+name],cwd=ROOT);assert digest(data)==expected
    dest=scratch/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(data)
   subprocess.run(['git','apply','--unsafe-paths','-'],input=gzip.decompress((folder/'runtime.patch.gz').read_bytes()),cwd=scratch,check=True)
   for name,expected in metadata['runtime_sources']['candidate'].items():
    assert digest((scratch/name).read_bytes())==expected,(label,name)
    if label=='inlined':assert digest((ROOT/name).read_bytes())==expected,name
   for name in ['domain-raw.jsonl.gz','domain-allocations.jsonl.gz','public-raw.jsonl.gz','public-allocations.jsonl.gz','measurement.json']:shutil.copyfile(folder/name,scratch/name)
   subprocess.run([sys.executable,str(folder/'analyze.py'),str(scratch)],check=True,stdout=subprocess.DEVNULL)
   assert (scratch/'summary.json').read_bytes()==gzip.decompress((folder/'summary.json.gz').read_bytes())
  summary=json.loads(gzip.decompress((folder/'summary.json.gz').read_bytes()))
  assert (summary['timed_encodings'],summary['timed_claims'],summary['timed_envelopes'])==(192000000,4608,222720)
  controls=[r for r in summary['domain_latency'] if r['path']=='key-control'];actions=[r for r in summary['domain_latency'] if r['path']=='action']
  assert len(controls)==len(actions)==40 and all(r['ci_high']<0 for r in actions)
  assert sum(r['ci_low']>0 for r in controls)==(12 if label=='initial' else 0)
  assert sum(r['ci_low']>0 for r in summary['public_latency'])==(1 if label=='initial' else 0)
  for row in summary['public_allocation']:
   multiplier=row['batch'] if not row['timers'] and row['length']==128 else 0
   for metric,factor in [('allocations',4),('requested_bytes',512),('reallocations',0)]:
    assert row['baseline'][metric]['median']-row['candidate'][metric]['median']==multiplier*factor,(label,row,metric)
  for variant in ['baseline','candidate']:
   text=log(folder,variant+'-tests');assert 'effect_key_legacy_cases=10005' in text and 'test result: ok.' in text
  assert 'heap-first kind=0' in log(folder,'baseline-guard') and 'left: (6, 0, 309)' in log(folder,'baseline-guard')
  assert 'action_effect_profiles=20 allocations=1 reallocations=0 exact_output=true' in log(folder,'candidate-guard')
 negative=json.loads((HERE/'negative-results.json').read_text());assert set(negative)=={'public-budget','encoding','integrity'} and all(code!=0 for code in negative.values())
 assert 'outbox: allocations=' in log(HERE,'negative-public-budget')
 assert 'test result: FAILED' in log(HERE,'negative-encoding')
 assert 'test result: FAILED' in log(HERE,'negative-integrity') and 'process_actions/identity' in log(HERE,'negative-integrity')
 final=log(HERE,'final-domain')
 assert 'action_effect_boundary_cases=160 coordinate_mutations=1440 zero_attempt=true max_attempt=true action_id_excluded=true' in final
 assert 'effect_key_legacy_cases=10005' in final and 'action_effect_profiles=20 allocations=1' in final and 'test result: FAILED' not in final
 for name in ['stable-workspace','msrv']:
  text=log(HERE,name);assert 'test result: FAILED' not in text and 'error:' not in text
  assert all(f'{count} passed; 0 failed' in text for count in [45,72])
  assert 'direct_action_encoding_preserves_boundaries_and_all_semantic_coordinates ... ok' in text
 for name in ['final-public-guard','msrv-public-guard']:assert 'public_claim_allocation_profiles=2 maximum_scope=true batch=128 verified=true reopen=true' in log(HERE,name)
 for name in ['clippy','msrv-check']:assert 'Finished `dev`' in log(HERE,name) and 'error:' not in log(HERE,name)
 assert not log(HERE,'fmt').strip() and not log(HERE,'layers').strip()
 qualification=json.loads((HERE/'qualification.json').read_text());assert qualification and all(row['exit_code']==0 for row in qualification.values())
 print('Verified both experiments: 384,000,000 encodings, 9,216 public claims, 1,088 allocation observations, parity, negative controls and qualification.')
if __name__=='__main__':main()
