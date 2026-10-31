from pathlib import Path
import hashlib,json,os,shutil,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent
runtime=['crates/penelope-domain/src/implementation.rs','crates/penelope-sqlite/src/leases.rs','crates/penelope-sqlite/src/process.rs','crates/penelope-executor/src/services.rs']
adapt=['crates/penelope-domain/benches/action_effect_key.rs','crates/penelope-domain/benches/action_effect_allocations.rs','crates/penelope-domain/tests/effect_key_encoding.rs','crates/penelope-domain/tests/action_effect_allocations.rs']
common=['crates/penelope-domain/tests/support/effect_key_fixture.rs','crates/penelope-domain/tests/support/action_effect_fixture.rs','crates/penelope-ports/benches/support/outcome_log_allocator.rs','crates/penelope-sqlite/benches/lease_claim_profiles.rs','crates/penelope-sqlite/benches/lease_claim_profile_allocations.rs','crates/penelope-sqlite/benches/support/lease_claim_fixture.rs','crates/penelope-sqlite/benches/support/task_read_allocator.rs']
original={name:(root/name).read_bytes() for name in runtime+adapt};parent=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert not (out/'metadata.json').exists()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
metadata={'parent':parent,'runtime_sources':{},'harness_sha256':{},'binaries':{},'common_sha256':{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in common},'rustc':subprocess.check_output(['rustc','-Vv'],text=True)}
try:
 for variant in ['baseline','candidate']:
  for name in runtime:(root/name).write_bytes(subprocess.check_output(['git','show',parent+':'+name]) if variant=='baseline' else original[name])
  for name in adapt:(root/name).write_bytes(original[name].replace(b'.canonical_effect_key_bytes()',b'.effect_key().canonical_bytes()') if variant=='baseline' else original[name])
  metadata['runtime_sources'][variant]={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in runtime}
  metadata['harness_sha256'][variant]={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in adapt}
  with (out/(variant+'-tests.txt')).open('w') as log:
   subprocess.run(['cargo','test','-p','penelope-domain','--all-features','--test','effect_key_encoding','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  with (out/(variant+'-guard.txt')).open('w') as log:
   result=subprocess.run(['cargo','test','-p','penelope-domain','--test','action_effect_allocations'],env=env,stdout=log,stderr=subprocess.STDOUT)
  assert (result.returncode==0)==(variant=='candidate')
  if variant=='baseline':assert 'heap-first kind=0' in (out/(variant+'-guard.txt')).read_text()
  for package,benches in [('penelope-domain',['action_effect_key','action_effect_allocations']),('penelope-sqlite',['lease_claim_profiles','lease_claim_profile_allocations'])]:
   command=['cargo','build','--release','-p',package,'--message-format=json']
   for bench in benches:command+=['--bench',bench]
   with (out/(variant+'-'+package+'-build.txt')).open('w') as log:result=subprocess.run(command,env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
   artifacts=[json.loads(line) for line in result.stdout.splitlines()]
   for bench in benches:
    binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==bench)
    dest=out/(variant+'-'+bench);shutil.copy2(binary,dest);metadata['binaries'][dest.name]=hashlib.sha256(dest.read_bytes()).hexdigest()
finally:
 for name,data in original.items():(root/name).write_bytes(data)
assert all((root/name).read_bytes()==data for name,data in original.items())
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
