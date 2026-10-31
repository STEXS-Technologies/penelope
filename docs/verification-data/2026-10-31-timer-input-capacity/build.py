from pathlib import Path
import hashlib,json,os,shutil,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent
runtime=['crates/penelope-executor/src/services.rs','crates/penelope-executor/src/timer_input.rs']
common=['crates/penelope-executor/benches/timer_input.rs','crates/penelope-executor/benches/timer_input_allocations.rs','crates/penelope-executor/tests/support/timer_input_fixture.rs','crates/penelope-executor/tests/support/timer_input_legacy.rs','crates/penelope-executor/tests/timer_input_identity.rs','crates/penelope-ports/benches/support/outcome_log_allocator.rs']
original={p:(root/p).read_bytes() for p in runtime};parent='00b79aaf84e59ce1a74fd2c6b75bd5dc099c0650'
oldservices=subprocess.check_output(['git','show',parent+':'+runtime[0]])
oldfn=oldservices[oldservices.index(b'/// Stable timer inbox input'):oldservices.index(b'\n/// Drains one authorized')].replace(b'fn timer_input(',b'pub(super) fn timer_input(')
legacy=(root/common[3]).read_bytes();assert legacy[legacy.index(b'/// Stable timer inbox input'):].strip()==oldfn.strip()
oldhelper=original[runtime[1]][:original[runtime[1]].index(b'/// Stable timer inbox input')]+oldfn
metadata={'parent':parent,'runtime_sources':{},'common_sha256':{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in common},'binaries':{},'rustc':subprocess.check_output(['rustc','-Vv'],text=True)}
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
assert not (out/'metadata.json').exists()
try:
 for variant in ['baseline','candidate']:
  for name in runtime:(root/name).write_bytes((oldservices if name==runtime[0] else oldhelper) if variant=='baseline' else original[name])
  metadata['runtime_sources'][variant]={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in runtime}
  with (out/(variant+'-tests.txt')).open('w') as log:subprocess.run(['cargo','test','-p','penelope-executor','--test','timer_input_identity','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  with (out/(variant+'-build.txt')).open('w') as log:result=subprocess.run(['cargo','build','--release','-p','penelope-executor','--message-format=json','--bench','timer_input','--bench','timer_input_allocations'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
  artifacts=[json.loads(line) for line in result.stdout.splitlines()]
  for bench in ['timer_input','timer_input_allocations']:
   binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==bench)
   dest=out/(variant+'-'+bench);shutil.copy2(binary,dest);metadata['binaries'][dest.name]=hashlib.sha256(dest.read_bytes()).hexdigest()
finally:
 for name,data in original.items():(root/name).write_bytes(data)
assert all((root/name).read_bytes()==data for name,data in original.items())
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
