"""Validate evidence without changing production source or archived observations."""
from pathlib import Path
import gzip,hashlib,json,shutil,subprocess,sys,tempfile
out=Path(__file__).resolve().parent;root=out.parents[2]
def read(directory,name):
 p=directory/name
 return p.read_bytes() if p.exists() else gzip.decompress((directory/(name+'.gz')).read_bytes())
def sha(data):return hashlib.sha256(data).hexdigest()
manifest=json.loads(read(out,'manifest.json'));files={p.relative_to(out).as_posix() for p in out.rglob('*') if p.is_file() and p.name!='manifest.json'};assert set(manifest)==files
for name,digest in manifest.items():assert sha((out/name).read_bytes())==digest,name
sources=json.loads(read(out,'source-hashes.json'))
for name,digest in sources.items():assert sha((root/name).read_bytes())==digest,name
parent='be823698205af8fdbbc8f855d95667375b0a12ff';services='crates/penelope-executor/src/services.rs';baseline=subprocess.check_output(['git','show',parent+':'+services],cwd=root)
summary_by_family={}
for family in ['initial','generic','final']:
 directory=out/family;m=json.loads(read(directory,'metadata.json'));assert m['parent']==parent
 overrides=json.loads(read(directory,'source-overrides.json'));assert set(overrides)<=set(m['common_sha256'])
 for name,digest in m['common_sha256'].items():
  data=gzip.decompress((directory/overrides[name]).read_bytes()) if name in overrides else (root/name).read_bytes()
  assert sha(data)==digest,(family,name)
 assert m['runtime_sources']['baseline']=={services:sha(baseline)}
 assert len(m['binaries'])==4 and all(len(h)==64 for h in m['binaries'].values())
 for variant in ['baseline','candidate']:
  text=read(directory,variant+'-tests.txt').decode()
  assert 'lease_batch_pair_cases=35664' in text and 'lease_batch_public_fault_cases=864' in text and '2 passed; 0 failed' in text
  assert 'lease_batch_allocation_profiles=18 allocations=0 reallocations=0 requested_bytes=0' in text
  text=read(directory,variant+'-build.txt').decode();assert 'Finished' in text and 'error:' not in text
 with tempfile.TemporaryDirectory(prefix='penelope-batch-evidence-') as folder:
  temporary=Path(folder);path=temporary/services;path.parent.mkdir(parents=True);path.write_bytes(baseline)
  subprocess.run(['patch','--batch','--forward','-p1','-d',str(temporary)],input=read(directory,'runtime.patch'),stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,check=True)
  assert sha(path.read_bytes())==m['runtime_sources']['candidate'][services]
  if family=='final':assert path.read_bytes()==(root/services).read_bytes()
  for name in ['raw.jsonl.gz','allocations.jsonl.gz','measurement.json']:shutil.copy2(directory/name,temporary/name)
  subprocess.run([sys.executable,str(directory/'analyze.py'),str(temporary)],stdout=subprocess.DEVNULL,check=True)
  assert (temporary/'summary.json').read_bytes()==read(directory,'summary.json')
  summary_by_family[family]=json.loads(read(directory,'summary.json'))
assert [sum(r['ci_low']>0 for r in summary_by_family[f]['latency']) for f in ['initial','generic','final']]==[13,51,0]
final=summary_by_family['final']['latency']
assert sum(r['ci_high']<0 for r in final if not r['timers'] and r['batch']>16)==36
assert sum(r['ci_high']<0 for r in final if r['timers'] and r['batch']>16)==27
for name in ['stable-workspace.txt','msrv.txt']:
 text=read(out/'final',name).decode();assert '0 failed' in text and 'test result: FAILED' not in text
 assert 'test earliest_conflict_matches_pair_scan_for_every_duplicate_pair ... ok' in text
 assert 'test public_batches_preserve_timeout_precedence_and_fail_before_delivery ... ok' in text
 assert 'lease_batch_allocation_profiles=18 allocations=0 reallocations=0 requested_bytes=0' in text
for name in ['final-clippy.txt','msrv-check.txt']:
 text=read(out/'final',name).decode();assert 'Finished' in text and 'error:' not in text
assert read(out/'final','fmt.txt')==b'' and read(out/'final','layers.txt')==b''
assert b'Code graph updated.' in read(out/'final','graph.txt')
negative=json.loads(read(out,'negative-results.json'));assert {n['name'] for n in negative}=={'action','token','precedence','short-validation','allocation','timer-wiring'}
for n in negative:assert n['rejected'] and n['marker'] in read(out,'negative-'+n['name']+'.txt').decode()
q=json.loads(read(out,'qualification.json'));assert q.pop('source_restored') is True and q.pop('negative_controls')==6 and all(value==0 for value in q.values())
print('service batch evidence verified: 3 experiments, 10,368 timing rows, 864 allocation observations, 35,664 pair cases, 864 public fault cases, 6 rejected negative controls')
