from pathlib import Path
import hashlib,json,os,shutil,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent
names=['crates/penelope-sqlite/src/leases.rs','crates/penelope-ports/src/implementation.rs']
original={name:(root/name).read_bytes() for name in names}
parent=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
pre_port='4fd5916a3a40c300f9f3975d0492f69ef5fcde5d'
assert not (out/'metadata.json').exists()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
harness=['crates/penelope-sqlite/benches/lease_claim_profiles.rs','crates/penelope-sqlite/benches/lease_claim_profile_allocations.rs','crates/penelope-sqlite/benches/support/lease_claim_fixture.rs','crates/penelope-sqlite/benches/support/task_read_allocator.rs']
metadata={'parent':parent,'pre_port':pre_port,'sources':{},'binaries':{},'harness_sha256':{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in harness},'rustc':subprocess.check_output(['rustc','-Vv'],text=True)}
try:
 for variant in ['pre-port','baseline','candidate']:
  for name in names:
   data=original[name]
   if name.endswith('/leases.rs') and variant!='candidate':data=subprocess.check_output(['git','show',parent+':'+name])
   if name.endswith('/implementation.rs') and variant=='pre-port':data=subprocess.check_output(['git','show',pre_port+':'+name])
   (root/name).write_bytes(data)
  metadata['sources'][variant]={name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in names}
  benches=['lease_claim_profiles','lease_claim_profile_allocations']
  command=['cargo','build','--release','-p','penelope-sqlite','--message-format=json']
  for bench in benches:command+=['--bench',bench]
  with (out/(variant+'-build.txt')).open('w') as log:
   result=subprocess.run(command,env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
  artifacts=[json.loads(line) for line in result.stdout.splitlines()]
  for bench in benches:
   binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==bench)
   dest=out/(variant+'-'+bench);shutil.copy2(binary,dest);metadata['binaries'][dest.name]=hashlib.sha256(dest.read_bytes()).hexdigest()
finally:
 for name,data in original.items():(root/name).write_bytes(data)
assert all((root/name).read_bytes()==data for name,data in original.items())
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
