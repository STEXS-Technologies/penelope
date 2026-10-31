from pathlib import Path
import hashlib,json,os,shutil,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent;parent='9fa82f60c39288b3512c1f1ece35077fb9b3b5cc'
runtime='crates/penelope-executor/src/timer_input.rs';path=root/runtime;original=path.read_bytes()
common=['crates/penelope-executor/src/services.rs','crates/penelope-executor/src/lease_batch_index.rs','crates/penelope-executor/benches/timer_input.rs','crates/penelope-executor/benches/timer_input_allocations.rs','crates/penelope-executor/tests/support/timer_input_fixture.rs','crates/penelope-executor/tests/support/timer_input_legacy.rs','crates/penelope-executor/tests/timer_input_identity.rs','crates/penelope-executor/tests/timer_input_allocations.rs','crates/penelope-executor/tests/timer_input_hex.rs','crates/penelope-executor/benches/lease_batch_services.rs','crates/penelope-executor/benches/lease_batch_service_allocations.rs','crates/penelope-executor/tests/support/lease_batch_fixture.rs','crates/penelope-ports/benches/support/outcome_log_allocator.rs','Cargo.lock']
sha=lambda b:hashlib.sha256(b).hexdigest();m={'parent':parent,'runtime_sources':{},'common_sha256':{p:sha((root/p).read_bytes()) for p in common},'binaries':{},'diagnostic_rustflags':'-C llvm-args=--align-all-functions=6','rustc':subprocess.check_output(['rustc','-Vv'],text=True)}
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1',RUSTFLAGS='-C llvm-args=--align-all-functions=6',CARGO_TARGET_DIR=str(root/'target/timer-hex-align-build'));env.pop('PENELOPE_TIMER_BATCH_CASE',None)
assert not (out/'metadata.json').exists()
try:
 for variant in ['baseline','candidate']:
  path.write_bytes(subprocess.check_output(['git','show',parent+':'+runtime]) if variant=='baseline' else original)
  m['runtime_sources'][variant]={runtime:sha(path.read_bytes())}
  tests=['timer_input_identity','timer_input_allocations']+(['timer_input_hex'] if variant=='candidate' else [])
  command=['cargo','test','-p','penelope-executor']
  for test in tests:command+=['--test',test]
  with (out/(variant+'-tests.txt')).open('w') as log:subprocess.run(command+['--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  benches=['timer_input','timer_input_allocations','lease_batch_services','lease_batch_service_allocations'];command=['cargo','build','--release','-p','penelope-executor','--message-format=json']
  for bench in benches:command+=['--bench',bench]
  with (out/(variant+'-build.txt')).open('w') as log:r=subprocess.run(command,env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
  artifacts=[json.loads(line) for line in r.stdout.splitlines()]
  for bench in benches:
   binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==bench)
   dest=out/(variant+'-'+bench);shutil.copy2(binary,dest);m['binaries'][dest.name]=sha(dest.read_bytes())
finally:path.write_bytes(original)
assert path.read_bytes()==original
(out/'metadata.json').write_text(json.dumps(m,indent=2)+'\n')
