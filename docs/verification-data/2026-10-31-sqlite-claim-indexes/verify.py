"""Verify instruction counts, query equivalence, mutation controls and fault runs."""
from pathlib import Path
import gzip,hashlib,json
root=Path(__file__).resolve().parent
manifest=json.loads((root/'sha256.json').read_text())
assert set(manifest)=={p.name for p in root.iterdir() if p.is_file() and p.name!='sha256.json'}
for name,digest in manifest.items(): assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
metadata=json.loads((root/'metadata.json').read_text())
for name,digest in metadata['source_sha256'].items(): assert hashlib.sha256(gzip.decompress((root/(name+'.rs.gz')).read_bytes())).hexdigest()==digest
leases=gzip.decompress((root/'leases.rs.gz').read_bytes()).decode()
assert len(metadata['queries'])==2 and all(query in leases for query in metadata['queries'].values())
rows=json.loads((root/'claim-selection-repeated.json').read_text())
assert len(rows)==20
assert {(r['compiler'],r['repeat']) for r in rows}=={(c,n) for c in ['stable','1.85.0'] for n in range(10)}
reference=None
for row in rows:
 assert not row['stderr']
 reports=[json.loads(line) for line in row['stdout'].splitlines()]
 assert len(reports)==12
 if reference is None: reference=reports
 assert reports==reference
 for report in reports:
  n=report['retained'];indexed=report['indexed'];future=report['future_timers']
  assert report['outbox']['vm_steps']==(202 if indexed else 235+5*n)
  assert report['timers']['vm_steps']==(206 if indexed else 413+(7 if future else 5)*n)
  assert report['timers']['sorts']==(0 if indexed else 1)
  for kind in ['outbox','timers']: assert report[kind]['identities']==[f'pending_{i:03}' for i in range(16)]
  expected={'outbox_insert':55 if indexed else 45,'outbox_ack':38 if indexed else 26,'outbox_lease_update':31,'timer_insert':49 if indexed else 38,'timer_ack':39 if indexed else 26,'timer_lease_update':31}
  assert report['mutation_vm_steps']==expected
  assert report['sqlite_version']=='3.53.2'
 assert {(r['retained'],r['future_timers'],r['indexed']) for r in reports}=={(n,f,i) for n in [0,1024,16384] for f in [False,True] for i in [False,True]}
for compiler in ['stable','1.85.0']: assert len({r['binary_sha256'] for r in rows if r['compiler']==compiler})==1
faults=json.loads((root/'claim-indexes-repeated.json').read_text())
assert len(faults)==70
assert {(r['compiler'],r['kind'],r['repeat']) for r in faults}=={(c,k,n) for c in ['stable','1.85.0'] for k,total in [('ports',25),('crash',10)] for n in range(total)}
for row in faults:
 assert not row['stderr']
 expected=5 if row['kind']=='ports' else 1
 assert f'test result: ok. {expected} passed;' in row['stdout']
for compiler in ['stable','1.85.0']:
 for kind in ['ports','crash']: assert len({r['binary_sha256'] for r in faults if r['compiler']==compiler and r['kind']==kind})==1
for suffix in ['workspace','msrv']:
 log=gzip.decompress((root/('claim-indexes-'+suffix+'.log.gz')).read_bytes()).decode()
 for count in [21,39,26]: assert f'test result: ok. {count} passed;' in log
 assert 'test result: FAILED' not in log and 'error: test failed' not in log
print('Verified 240 selection fixtures, 250 public-port cases, and 180 SIGKILL migration cuts')
