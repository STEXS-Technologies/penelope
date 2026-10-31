"""Counterfactual: run candidate regressions against only the pinned old worker."""
from pathlib import Path
import os,resource,subprocess
out=Path(__file__).resolve().parent
source=Path('crates/penelope-sqlite/src/worker.rs')
fixed=source.read_bytes()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
resource.setrlimit(resource.RLIMIT_CORE,(0,0))
try:
 source.write_bytes(subprocess.check_output(['git','show','8e4e2254182282fb9ae79a52bd8b17cc8cede68a:crates/penelope-sqlite/src/worker.rs']))
 with (out/'baseline-control.log').open('w') as log:
  result=subprocess.run(['cargo','test','-p','penelope-sqlite','--lib','worker::panic_tests::','--','--nocapture','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT)
 text=(out/'baseline-control.log').read_text()
 assert result.returncode==101
 assert 'test result: FAILED. 4 passed; 2 failed' in text
 for kind in ['full','disconnected']:
  for nested in ['false','true']:
   assert f'{kind} nested={nested} unwound=true dropped_once=true not_executed=true' in text
finally:
 source.write_bytes(fixed)
for compiler in ['stable','1.85.0']:
 local=dict(env)
 if compiler=='1.85.0':local['CARGO_TARGET_DIR']='target/worker-audit-msrv'
 with (out/f'final-{compiler}-sqlite.log').open('w') as log:
  subprocess.run(['cargo','+'+compiler,'test','-p','penelope-sqlite','--lib','--test','process','--test','tasks','--all-features','--','--test-threads=1'],env=local,stdout=log,stderr=subprocess.STDOUT,check=True)
 for n in [31,51,42]:assert f'test result: ok. {n} passed; 0 failed' in (out/f'final-{compiler}-sqlite.log').read_text()
print('Baseline: four rejected callbacks unwound; restored implementation: both complete SQLite suites passed.')
