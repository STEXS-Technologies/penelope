"""Prove the scope regression rejects a query that ignores its scope binding."""
from pathlib import Path
import os,subprocess
src=Path('crates/penelope-sqlite/src/tasks.rs');original=src.read_bytes()
old=b'const BLOB_QUERY: &str = "SELECT bytes FROM task_blobs WHERE scope=?1 AND digest=?2";'
new=b'const BLOB_QUERY: &str = "SELECT bytes FROM task_blobs WHERE ?1 IS NOT NULL AND digest=?2";'
assert original.count(old)==1
out=Path(__file__).resolve().parent
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
try:
 src.write_bytes(original.replace(old,new))
 with (out/'negative-control.log').open('w') as log:
  r=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','tasks','task_statement_cache::','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
 assert r.returncode==101
 log=(out/'negative-control.log').read_text();assert 'test result: FAILED' in log and 'left: Ok(5)' in log and 'right: Err(Corrupt)' in log
 print('Ignoring the scope binding incorrectly serves another scope; regression rejects the fault.')
finally:src.write_bytes(original)
for compiler in ['stable','1.85.0']:
 final_env=dict(env)
 if compiler=='1.85.0':final_env['CARGO_TARGET_DIR']='target/worker-audit-msrv'
 with (out/f'final-{compiler}-scope.log').open('w') as log:
  subprocess.run(['cargo','+'+compiler,'test','-p','penelope-sqlite','--test','tasks','task_statement_cache::','--','--nocapture'],env=final_env,stdout=log,stderr=subprocess.STDOUT,check=True)
print('Production source restored; stable/MSRV scope regression passed.')
