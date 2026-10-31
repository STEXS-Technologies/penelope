"""Check expanded rollback test against old code, restore, then qualify SQLite."""
from pathlib import Path
import os,subprocess
out=Path(__file__).resolve().parent
source=Path('crates/penelope-sqlite/src/leases.rs');fixed=source.read_bytes()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
try:
 source.write_bytes(subprocess.check_output(['git','show','4b63eac65464123cce5bff3665e59272608a4244:crates/penelope-sqlite/src/leases.rs']))
 with (out/'baseline-compatibility.log').open('w') as log:
  subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','warmed_token_statements_preserve_batch_rollback_retry_and_reopen','--','--nocapture','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
finally:source.write_bytes(fixed)
for compiler in ['stable','1.85.0']:
 local=dict(env)
 if compiler=='1.85.0':local['CARGO_TARGET_DIR']='target/worker-audit-msrv'
 with (out/f'final-{compiler}-sqlite.log').open('w') as log:
  subprocess.run(['cargo','+'+compiler,'test','-p','penelope-sqlite','--lib','--test','process','--test','tasks','--all-features','--','--test-threads=1'],env=local,stdout=log,stderr=subprocess.STDOUT,check=True)
 for n in [31,51,42]:assert f'test result: ok. {n} passed; 0 failed' in (out/f'final-{compiler}-sqlite.log').read_text()
print('Expanded test passes baseline; source restored and complete stable/MSRV SQLite suites pass.')
