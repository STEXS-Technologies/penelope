"""Read-only verification of public SQLite borrowed claim scope evidence."""
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
def log(name):return gzip.decompress((HERE/(name+'.txt.gz')).read_bytes()).decode()
def main():
 actual={str(p.relative_to(HERE)):digest(p.read_bytes()) for p in HERE.rglob('*') if p.is_file() and p.name!='manifest.json'}
 assert actual==json.loads((HERE/'manifest.json').read_text())
 for name,expected in json.loads((HERE/'source-hashes.json').read_text()).items():assert digest((ROOT/name).read_bytes())==expected,name
 metadata=json.loads((HERE/'metadata.json').read_text())
 for name,expected in metadata['harness_sha256'].items():assert digest((ROOT/name).read_bytes())==expected,name
 for variant,sources in metadata['sources'].items():
  for name,expected in sources.items():
   if variant=='candidate':data=(ROOT/name).read_bytes()
   else:
    parent=metadata['pre_port'] if variant=='pre-port' and name.startswith('crates/penelope-ports/') else metadata['parent']
    data=subprocess.check_output(['git','show',parent+':'+name],cwd=ROOT)
   assert digest(data)==expected,(variant,name)
 with tempfile.TemporaryDirectory() as temporary:
  scratch=Path(temporary);name='crates/penelope-sqlite/src/leases.rs';source=scratch/name;source.parent.mkdir(parents=True)
  source.write_bytes(subprocess.check_output(['git','show',metadata['parent']+':'+name],cwd=ROOT))
  subprocess.run(['git','apply','--unsafe-paths','-'],input=gzip.decompress((HERE/'runtime.patch.gz').read_bytes()),cwd=scratch,check=True)
  assert digest(source.read_bytes())==metadata['sources']['candidate'][name]
  for name in ['raw.jsonl.gz','allocations.jsonl.gz','measurement.json']:shutil.copyfile(HERE/name,scratch/name)
  subprocess.run([sys.executable,str(HERE/'analyze.py'),str(scratch)],check=True,stdout=subprocess.DEVNULL)
  assert (scratch/'summary.json').read_bytes()==(HERE/'summary.json').read_bytes()
 summary=json.loads((HERE/'summary.json').read_text())
 assert summary['timed_claims']==4608 and summary['timed_envelopes']==222720
 assert len(summary['latency'])==24 and not any(row['ci_low']>0 for row in summary['latency'])
 assert sum(row['ci_high']<0 for row in summary['latency'])==4
 negative=json.loads((HERE/'negative-results.json').read_text())
 assert set(negative)=={'outbox','timer','tenant_id','process_id','definition_id','definition_version','definition_digest'}
 for name,code in negative.items():
  assert code!=0
  text=log('negative-'+name)
  if name in ['outbox','timer']:assert name+': allocations=' in text
  else:assert 'test result: FAILED' in text and 'left: true' in text
 assert 'borrowed_claim_scope_cases=128 fixture_profiles=32 all_coordinates=true' in log('focused')
 for name in ['stable-workspace','msrv']:
  text=log(name);assert 'test result: FAILED' not in text and 'error:' not in text
  assert all(f'{count} passed; 0 failed' in text for count in [45,72])
 for name in ['final-guards','msrv-guards']:assert 'public_claim_allocation_profiles=2 maximum_scope=true batch=128 verified=true reopen=true' in log(name)
 for name in ['clippy','msrv-check']:assert 'Finished `dev`' in log(name) and 'error:' not in log(name)
 assert not log('fmt').strip() and not log('layers').strip()
 qualification=json.loads((HERE/'qualification.json').read_text());assert qualification and all(row['exit_code']==0 for row in qualification.values())
 print('Verified 4,608 public claims, 576 allocation observations, seven negative controls, source identity and qualification.')
if __name__=='__main__':main()
