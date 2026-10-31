"""Check the storage/CPU diagnostic without rerunning profiling or timing."""
from pathlib import Path
import gzip,hashlib,json,re,shutil,subprocess,sys,tempfile
out=Path(__file__).resolve().parent;root=out.parents[2]
def read(folder,name):
 p=folder/name
 return p.read_bytes() if p.exists() else gzip.decompress((folder/(name+'.gz')).read_bytes())
def sha(data):return hashlib.sha256(data).hexdigest()
manifest=json.loads((out/'manifest.json').read_text())
assert set(manifest)=={p.relative_to(out).as_posix() for p in out.rglob('*') if p.is_file() and p.name!='manifest.json'}
for name,digest in manifest.items():assert sha((out/name).read_bytes())==digest,name
audit=json.loads(read(out,'audit.json'));assert audit['parent']=='42e09f852e6d745123a4f0d44796a1a00c8baed9' and audit['no_production_changes']
for family in ['btrfs','tmpfs-exploratory']:
 directory=out/family;m=json.loads(read(directory,'metadata.json'))
 assert m['binary_sha256']==audit['binary_sha256']==m['source_metadata']['binary_sha256']
 assert m['cpus']=={'one':[0],'two':[0,1]}
 assert ('btrfs' in m['filesystem'])==(family=='btrfs')
 if family=='btrfs':assert m['database_root'].endswith('/target/durable-timer-affinity/databases')
 else:assert m['database_root']=='/tmp' and 'not this diagnostic storage' in m['note']
 for path,digest in m['source_metadata']['source_sha256'].items():
  assert sha((root/path).read_bytes())==digest,path
  assert sha(subprocess.check_output(['git','show',audit['parent']+':'+path],cwd=root))==digest,path
 with tempfile.TemporaryDirectory(prefix='penelope-timer-storage-') as temporary:
  destination=Path(temporary);shutil.copy2(directory/'raw.jsonl.gz',destination/'raw.jsonl.gz')
  subprocess.run([sys.executable,str(directory/'analyze.py'),str(destination)],check=True,stdout=subprocess.DEVNULL)
  assert (destination/'summary.json').read_bytes()==read(directory,'summary.json')
 summary=json.loads(read(directory,'summary.json'));assert summary['rows']==96 and summary['timers']==12288
 if family=='btrfs':
  rows=[r for r in summary['latency'] if r['metric']=='managed_ns'];assert len(rows)==2 and all(r['ci_low']<0<r['ci_high'] for r in rows)
 trace=read(directory,'syscalls.txt').decode();assert 'fsync' in trace and 'futex' in trace and 'pwrite64' in trace
 row=json.loads(read(directory,'syscall-row.jsonl'));assert row['verified'] and row['consumed']==128 and row['batch']==128 and row['width']==128 and row['work']==0
 assert row['reopen']==(family=='tmpfs-exploratory')
 if family=='btrfs':assert re.search(r'\b571\s+794\s+fsync\b',trace)
assert subprocess.check_output(['git','diff',audit['parent'],'--',':(glob)crates/*/src/**'],cwd=root)==b'', 'production source changes require the pinned parent checkout'
assert audit['btrfs_profile']['database_root'].endswith('/target/durable-timer-affinity/databases')
assert audit['btrfs_profile']['cpus']==[0] and audit['btrfs_profile']['reopen'] is False
assert audit['tmpfs_profile']['database_root']=='/tmp' and audit['tmpfs_profile']['reopen'] is True
for path,digest in json.loads(read(out,'source-hashes.json')).items():assert sha((root/path).read_bytes())==digest,path
print('timer storage audit verified: 192 ordinary workflows, 24,576 timers, 2 exact paired summaries, separate exploratory tmpfs data, Btrfs diagnostic fsync count, unchanged workload source')
