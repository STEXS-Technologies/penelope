from pathlib import Path
import os,sys,subprocess,json,hashlib,shutil
root=Path.cwd();out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
out.mkdir(parents=True,exist_ok=True)
if (out/'build-metadata.json').exists():raise FileExistsError('use a fresh output directory')
parent='a8f79b4c3ba178ba1865ea1165f9e644767afddb';source='crates/penelope-ports/src/implementation.rs';p=root/source;original=p.read_bytes()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',RUST_TEST_THREADS='1',TMPDIR=str(root/'target/qualification-temp'))
harness=['crates/penelope-ports/benches/outcome_log.rs','crates/penelope-ports/benches/outcome_log_allocations.rs','crates/penelope-ports/benches/support/outcome_log_fixture.rs','crates/penelope-ports/benches/support/outcome_log_allocator.rs']
metadata={'source_parent':parent,'sources':{},'binaries':{},'harness_sha256':{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in harness},'rustc':subprocess.check_output(['rustc','-Vv'],text=True)}
try:
 for label in ['baseline','candidate']:
  p.write_bytes(subprocess.check_output(['git','show',f'{parent}:{source}']) if label=='baseline' else original)
  metadata['sources'][label]={source:hashlib.sha256(p.read_bytes()).hexdigest()}
  with (out/f'{label}-tests.txt').open('w') as log:
   subprocess.run(['cargo','test','-p','penelope-ports','--lib','--test','outcome_log_scope','--all-features'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  if label=='baseline':
   with (out/'allocation-negative-control.txt').open('w') as log:
    r=subprocess.run(['cargo','test','-p','penelope-ports','--test','outcome_log_allocations'],env=env,stdout=log,stderr=subprocess.STDOUT)
   assert r.returncode!=0
   assert 'scope rejection allocated 5 times' in (out/'allocation-negative-control.txt').read_text()
  with (out/f'{label}-build.txt').open('w') as log:
   r=subprocess.run(['cargo','build','--release','-p','penelope-ports','--bench','outcome_log','--bench','outcome_log_allocations','--message-format=json'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
  artifacts=[json.loads(line) for line in r.stdout.splitlines()]
  for bench in ['outcome_log','outcome_log_allocations']:
   binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==bench)
   dest=out/f'{label}-{bench}';shutil.copy2(binary,dest);metadata['binaries'][dest.name]=hashlib.sha256(dest.read_bytes()).hexdigest()
finally:p.write_bytes(original)
(out/'build-metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
