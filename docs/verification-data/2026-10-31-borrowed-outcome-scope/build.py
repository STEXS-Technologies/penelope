import sys
from pathlib import Path
import subprocess,os,json,hashlib,shutil
root=Path.cwd();out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent;parent='cbfe5c5f0d840cbd1ad2a4ba119a5883a9ab1c13'
out.mkdir(parents=True,exist_ok=True)
if (out/'build-metadata.json').exists(): raise FileExistsError('choose a fresh output directory')
sources=['crates/penelope-domain/src/implementation.rs','crates/penelope-sqlite/src/process.rs']
original={p:(root/p).read_bytes() for p in sources}
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',RUST_TEST_THREADS='1',TMPDIR=str(root/'target/qualification-temp'))
metadata={'source_parent':parent,'sources':{},'binaries':{},'harness_sha256':{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in ['crates/penelope-sqlite/benches/process_replay.rs','crates/penelope-sqlite/benches/process_replay_allocations.rs','crates/penelope-sqlite/benches/support/process_replay_fixture.rs']}}
try:
 for label in ['baseline','candidate']:
  for p in sources:
   content=subprocess.check_output(['git','show',f'{parent}:{p}']) if label=='baseline' else original[p]
   (root/p).write_bytes(content)
  metadata['sources'][label]={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in sources}
  with (out/f'{label}-process-tests.txt').open('w') as log:
   subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','--all-features'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  if label=='baseline':
   with (out/'allocation-negative-control.txt').open('w') as log:
    r=subprocess.run(['cargo','test','-p','penelope-domain','--test','identifier_allocations'],env=env,stdout=log,stderr=subprocess.STDOUT)
   assert r.returncode != 0
   assert 'panicked' in (out/'allocation-negative-control.txt').read_text()
  with (out/f'{label}-build.txt').open('w') as log:
   r=subprocess.run(['cargo','build','--release','-p','penelope-sqlite','--bench','process_replay','--bench','process_replay_allocations','--message-format=json'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
  artifacts=[json.loads(line) for line in r.stdout.splitlines()]
  for bench in ['process_replay','process_replay_allocations']:
   src=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==bench)
   dest=out/f'{label}-{bench}';shutil.copy2(src,dest);metadata['binaries'][dest.name]=hashlib.sha256(dest.read_bytes()).hexdigest()
finally:
 for p,b in original.items(): (root/p).write_bytes(b)
(out/'build-metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
