from pathlib import Path
import hashlib,json,os,shutil,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent;source=root/'crates/penelope-domain/src/implementation.rs';original=source.read_bytes();parent=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert not (out/'metadata.json').exists()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
harness=['crates/penelope-domain/benches/effect_key_encoding.rs','crates/penelope-domain/benches/effect_key_allocations.rs','crates/penelope-domain/tests/support/effect_key_fixture.rs','crates/penelope-ports/benches/support/outcome_log_allocator.rs','crates/penelope-sqlite/benches/lease_completion_allocations.rs','crates/penelope-sqlite/benches/support/task_read_allocator.rs']
metadata={'parent':parent,'source':str(source.relative_to(root)),'sources':{},'binaries':{},'harness_sha256':{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in harness},'rustc':subprocess.check_output(['rustc','-Vv'],text=True)}
try:
 for variant in ['baseline','candidate']:
  source.write_bytes(subprocess.check_output(['git','show',parent+':'+metadata['source']]) if variant=='baseline' else original)
  metadata['sources'][variant]=hashlib.sha256(source.read_bytes()).hexdigest()
  with (out/(variant+'-tests.txt')).open('w') as log:
   subprocess.run(['cargo','test','-p','penelope-domain','--all-features','--test','effect_key_encoding','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  with (out/(variant+'-guard.txt')).open('w') as log:
   result=subprocess.run(['cargo','test','-p','penelope-domain','--all-features','--test','identifier_allocations'],env=env,stdout=log,stderr=subprocess.STDOUT)
  assert (result.returncode==0)==(variant=='candidate')
  if variant=='baseline':assert 'short kind=0' in (out/(variant+'-guard.txt')).read_text()
  for package,names in [('penelope-domain',['effect_key_encoding','effect_key_allocations']),('penelope-sqlite',['lease_completion_allocations'])]:
   command=['cargo','build','--release','-p',package,'--message-format=json']
   for name in names:command+=['--bench',name]
   with (out/(variant+'-'+package+'-build.txt')).open('w') as log:
    result=subprocess.run(command,env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
   artifacts=[json.loads(line) for line in result.stdout.splitlines()]
   for name in names:
    binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==name)
    dest=out/(variant+'-'+name);shutil.copy2(binary,dest);metadata['binaries'][dest.name]=hashlib.sha256(dest.read_bytes()).hexdigest()
finally:source.write_bytes(original)
assert source.read_bytes()==original
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
