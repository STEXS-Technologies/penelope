"""Validate the focused repeat, including rejection of malformed observations."""
from pathlib import Path
import copy,gzip,hashlib,json,shutil,subprocess,sys,tempfile
out=Path(__file__).resolve().parent;root=out.parents[2]
def sha(data):return hashlib.sha256(data).hexdigest()
def read(name):
 p=out/name
 return p.read_bytes() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes())
manifest=json.loads(read('manifest.json'))
assert set(manifest)=={p.relative_to(out).as_posix() for p in out.rglob('*') if p.is_file() and p.name!='manifest.json'}
for name,digest in manifest.items():assert sha((out/name).read_bytes())==digest,name
for path,digest in json.loads(read('source-hashes.json')).items():assert sha((root/path).read_bytes())==digest,path
previous=root/'docs/verification-data/2026-10-31-inbox-retry-fastpath';metadata=json.loads((previous/'metadata.json').read_text());measurement=json.loads(read('measurement.json'))
assert measurement['binaries']=={name:digest for name,digest in metadata['binaries'].items() if name.endswith('-durable_timer_workload')}
assert measurement['cpu']==0 and measurement['allocation_processes']==0 and measurement['ordinary_processes']==96
assert measurement['start']<measurement['end'] and 'btrfs' in measurement['filesystem']
assert (root/'crates/penelope-sqlite/src/process.rs').read_bytes()==(previous/'candidate-process.rs').read_bytes()
with tempfile.TemporaryDirectory(prefix='penelope-inbox-schedule-') as folder:
 temporary=Path(folder)
 shutil.copy2(out/'measurement.json',temporary/'measurement.json');shutil.copy2(out/'raw.jsonl.gz',temporary/'raw.jsonl.gz')
 command=[sys.executable,str(out/'analyze.py'),'public',str(temporary)]
 subprocess.run(command,check=True,stdout=subprocess.DEVNULL)
 assert (temporary/'summary.json').read_bytes()==read('summary.json')
 rows=[json.loads(line) for line in read('raw.jsonl').decode().splitlines()]
 for fault in ['missing','duplicate','phase-sum','verification','checksum']:
  mutated=copy.deepcopy(rows)
  if fault=='missing':mutated.pop()
  elif fault=='duplicate':mutated[-1]=copy.deepcopy(mutated[0])
  elif fault=='phase-sum':mutated[0]['schedule_ns']+=1
  elif fault=='verification':mutated[0]['verified']=False
  elif fault=='checksum':mutated[0]['checksum']='different'
  (temporary/'raw.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in mutated))
  result=subprocess.run(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
  assert result.returncode and b'AssertionError' in result.stderr,fault
summary=json.loads(read('summary.json'));assert summary['rows']==192 and summary['timers']==3072 and len(summary['latency'])==10
for metric in ['managed_ns','schedule_ns']:
 rows=[r for r in summary['latency'] if r['metric']==metric];assert len(rows)==2 and all(r['ci_low']<0<r['ci_high'] for r in rows)
assert sum(r['ci_high']<0 for r in summary['latency'])==1 and all(r['ci_low']<0 for r in summary['latency'])
print('scheduling follow-up verified: 192 workflows/3,072 timers, identical executable hashes, exact paired summaries, five malformed datasets rejected; no supported scheduling or total-workflow regression in this repeat')
