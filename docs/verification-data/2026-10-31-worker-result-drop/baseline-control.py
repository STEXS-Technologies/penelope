"""Isolate old-code panic controls, suppress cores, restore and qualify."""
from pathlib import Path
import os,resource,subprocess
out=Path(__file__).resolve().parent;src=Path('crates/penelope-sqlite/src/worker.rs');original=src.read_bytes()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
resource.setrlimit(resource.RLIMIT_CORE,(0,0))
try:
 src.write_bytes(subprocess.check_output(['git','show','a0a7a6c69b04cca7a1056ab5eecf65df16db40f2:crates/penelope-sqlite/src/worker.rs']))
 for name,test in [('plain','canceled_result_destructor_cannot_strand_accepted_work'),('nested','nested_result_destructor_cannot_abort_worker_or_strand_accepted_work')]:
  with (out/f'baseline-{name}.log').open('w') as log:r=subprocess.run(['cargo','test','-p','penelope-sqlite','--lib','worker::panic_tests::'+test,'--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
  assert r.returncode==101
  log=(out/f'baseline-{name}.log').read_text()
  if name=='plain':assert 'left: Err(TimedOut)' in log and 'right: Ok(2)' in log and 'test result: FAILED' in log
  else:assert 'SIGABRT' in log and 'thread result panicked on drop' in log
  print('baseline',name,'expected failure observed',flush=True)
finally:src.write_bytes(original)
for compiler in ['stable','1.85.0']:
 final_env=dict(env,RUST_TEST_THREADS='1')
 if compiler=='1.85.0':final_env['CARGO_TARGET_DIR']='target/worker-audit-msrv'
 with (out/f'final-{compiler}-sqlite.log').open('w') as log:subprocess.run(['cargo','+'+compiler,'test','-p','penelope-sqlite','--lib','--test','tasks','--test','process'],env=final_env,stdout=log,stderr=subprocess.STDOUT,check=True)
 print(compiler,'restored SQLite suites passed',flush=True)
