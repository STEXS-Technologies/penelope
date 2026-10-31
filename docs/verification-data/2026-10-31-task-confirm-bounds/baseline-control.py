"""Run the regression against the pinned old source, then restore and qualify."""
from pathlib import Path
import subprocess,os
out=Path(__file__).resolve().parent;src=Path('crates/penelope-sqlite/src/authority.rs');original=src.read_bytes()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
try:
 src.write_bytes(subprocess.check_output(['git','show','eaa920ad8a2ecd458a18087cdecf1adad07cb1f6:crates/penelope-sqlite/src/authority.rs']))
 with (out/'baseline-control.log').open('w') as log:r=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','tasks','task_confirm_bounds::','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
 assert r.returncode==101
 log=(out/'baseline-control.log').read_text();assert 'test result: FAILED' in log and 'left: Ok(())' in log and 'right: Err(Conflict)' in log
 print('Pinned old implementation confirms an out-of-policy generation.',flush=True)
finally:src.write_bytes(original)
for compiler in ['stable','1.85.0']:
 final_env=dict(env,RUST_TEST_THREADS='1')
 if compiler=='1.85.0':final_env['CARGO_TARGET_DIR']='target/worker-audit-msrv'
 with (out/f'final-{compiler}-sqlite.log').open('w') as log:subprocess.run(['cargo','+'+compiler,'test','-p','penelope-sqlite','--lib','--test','tasks','--test','process'],env=final_env,stdout=log,stderr=subprocess.STDOUT,check=True)
 print(compiler,'final SQLite suites passed',flush=True)
