from pathlib import Path
import hashlib,json,os,shutil,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent;parent='be823698205af8fdbbc8f855d95667375b0a12ff'
runtime='crates/penelope-executor/src/services.rs';path=root/runtime;original=path.read_bytes()
common=['crates/penelope-executor/src/lease_batch_index.rs','crates/penelope-executor/tests/support/lease_batch_fixture.rs','crates/penelope-executor/tests/lease_batch_identity.rs','crates/penelope-executor/tests/lease_batch_allocations.rs','crates/penelope-executor/benches/lease_batch_services.rs','crates/penelope-executor/benches/lease_batch_service_allocations.rs','crates/penelope-ports/benches/support/outcome_log_allocator.rs','Cargo.lock']
sha=lambda b:hashlib.sha256(b).hexdigest()
m={'parent':parent,'runtime_sources':{},'common_sha256':{p:sha((root/p).read_bytes()) for p in common},'binaries':{},'rustc':subprocess.check_output(['rustc','-Vv'],text=True)}
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
assert not (out/'metadata.json').exists()
try:
 for variant in ['baseline','candidate']:
  path.write_bytes(subprocess.check_output(['git','show',parent+':'+runtime]) if variant=='baseline' else original)
  m['runtime_sources'][variant]={runtime:sha(path.read_bytes())}
  with (out/(variant+'-tests.txt')).open('w') as log:subprocess.run(['cargo','test','-p','penelope-executor','--test','lease_batch_identity','--test','lease_batch_allocations','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  with (out/(variant+'-build.txt')).open('w') as log:r=subprocess.run(['cargo','build','--release','-p','penelope-executor','--message-format=json','--bench','lease_batch_services','--bench','lease_batch_service_allocations'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
  artifacts=[json.loads(line) for line in r.stdout.splitlines()]
  for bench in ['lease_batch_services','lease_batch_service_allocations']:
   binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==bench)
   dest=out/(variant+'-'+bench);shutil.copy2(binary,dest);m['binaries'][dest.name]=sha(dest.read_bytes())
finally:path.write_bytes(original)
assert path.read_bytes()==original
(out/'metadata.json').write_text(json.dumps(m,indent=2)+'\n')
