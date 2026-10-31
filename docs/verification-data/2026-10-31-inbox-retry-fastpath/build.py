from pathlib import Path
import hashlib,json,os,shutil,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent;parent='8c11d33030c38075421a03f8f17f513d6dfe0d3d';runtime='crates/penelope-sqlite/src/process.rs';path=root/runtime;candidate=path.read_bytes()
sha=lambda b:hashlib.sha256(b).hexdigest()
common=['Cargo.lock', 'crates/penelope-sqlite/Cargo.toml', 'crates/penelope-sqlite/tests/process.rs', 'crates/penelope-sqlite/tests/support/inbox_retry_lock.rs', 'crates/penelope-sqlite/tests/support/inbox_validation.rs', 'crates/penelope-sqlite/tests/support/write_snapshot.rs', 'crates/penelope-sqlite/src/inbox_retry_tests.rs', 'crates/penelope-sqlite/benches/inbox_accept.rs', 'crates/penelope-sqlite/benches/inbox_accept_allocations.rs', 'crates/penelope-sqlite/benches/support/inbox_fixture.rs', 'crates/penelope-sqlite/benches/support/task_read_allocator.rs', 'crates/penelope-executor/benches/durable_timer_workload.rs', 'crates/penelope-executor/benches/support/timer_workload/workflow.rs', 'crates/penelope-executor/benches/support/timer_workload/ports.rs', 'crates/penelope-executor/tests/support/timer_input_legacy.rs']
m=dict(parent=parent,runtime_sources={},common_sha256={p:sha((root/p).read_bytes()) for p in common},binaries={},rustc=subprocess.check_output(['rustc','-Vv'],text=True))
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
assert not (out/'metadata.json').exists()
try:
 for variant in ['baseline','candidate']:
  data=subprocess.check_output(['git','show',parent+':'+runtime]) if variant=='baseline' else candidate
  path.write_bytes(data);(out/(variant+'-process.rs')).write_bytes(data);m['runtime_sources'][variant]={runtime:sha(data)}
  command=['cargo','test','--locked','-p','penelope-sqlite','--test','process','--all-features','--','--nocapture']
  if variant=='baseline':command+=['--skip','inbox_retry_lock','--skip','inbox_validation::duplicate_admission_validates_evidence_without_the_writer_lock']
  with (out/(variant+'-tests.txt')).open('w') as log:subprocess.run(command,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  if variant=='candidate':
   with (out/'candidate-race.txt').open('w') as log:subprocess.run(['cargo','test','--locked','-p','penelope-sqlite','--lib','concurrent_admission_is_rechecked','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  for package,benches in [('penelope-sqlite',['inbox_accept','inbox_accept_allocations']),('penelope-executor',['durable_timer_workload'])]:
   command=['cargo','build','--release','--locked','-p',package,'--message-format=json']
   for bench in benches:command+=['--bench',bench]
   with (out/(variant+'-'+package+'-build.txt')).open('w') as log:r=subprocess.run(command,env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
   artifacts=[json.loads(line) for line in r.stdout.splitlines()]
   for bench in benches:
    binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==bench)
    dest=out/(variant+'-'+bench);shutil.copy2(binary,dest);m['binaries'][dest.name]=sha(dest.read_bytes())
finally:path.write_bytes(candidate)
assert path.read_bytes()==candidate
(out/'metadata.json').write_text(json.dumps(m,indent=2)+'\n')
