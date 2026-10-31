from pathlib import Path
import hashlib,json,os,shutil,subprocess,sys
root=Path.cwd();out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
out.mkdir(parents=True,exist_ok=True)
if (out/'metadata.json').exists():raise FileExistsError('use a fresh output directory')
parent='2c9fb0c1d39dfb0d2e9ea8e0d585acb5b33f214a';name='crates/penelope-ports/src/implementation.rs';source=root/name;original=source.read_bytes()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',RUST_TEST_THREADS='1',TMPDIR=str(root/'target/qualification-temp'))
harness=['crates/penelope-ports/benches/atomic_validation.rs','crates/penelope-ports/benches/atomic_validation_allocations.rs','crates/penelope-ports/tests/support/atomic_validation_fixture.rs']
metadata={'source_parent':parent,'sources':{},'harness_sha256':{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in harness},'binaries':{},'rustc':subprocess.check_output(['rustc','-Vv'],text=True)}
try:
 for label in ['baseline','candidate']:
  source.write_bytes(subprocess.check_output(['git','show',parent+':'+name]) if label=='baseline' else original)
  metadata['sources'][label]={name:hashlib.sha256(source.read_bytes()).hexdigest()}
  with (out/(label+'-tests.txt')).open('w') as log:
   subprocess.run(['cargo','test','-p','penelope-ports','--test','atomic_validation_parity','--test','atomic_outcome_scope','--all-features','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  if label=='baseline':
   with (out/'negative-control.txt').open('w') as log:
    result=subprocess.run(['cargo','test','-p','penelope-ports','--test','atomic_scope_allocations','--all-features'],env=env,stdout=log,stderr=subprocess.STDOUT)
   assert result.returncode!=0
   assert 'atomic scope validation allocated: long=true' in (out/'negative-control.txt').read_text()
  with (out/(label+'-build.txt')).open('w') as log:
   result=subprocess.run(['cargo','build','--release','-p','penelope-ports','--bench','atomic_validation','--bench','atomic_validation_allocations','--message-format=json'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
  artifacts=[json.loads(line) for line in result.stdout.splitlines()]
  for bench in ['atomic_validation','atomic_validation_allocations']:
   binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==bench)
   destination=out/(label+'-'+bench);shutil.copy2(binary,destination)
   metadata['binaries'][destination.name]=hashlib.sha256(destination.read_bytes()).hexdigest()
finally:source.write_bytes(original)
assert source.read_bytes()==original
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
