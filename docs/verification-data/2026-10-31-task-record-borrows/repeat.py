from pathlib import Path
import os,subprocess
out=Path(__file__).resolve().parent
root=Path.cwd()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
with (out/'repeat.log').open('w') as log:
 for compiler,extra in [('stable',{}),('1.85.0',{'CARGO_TARGET_DIR':'target/worker-audit-msrv'})]:
  for n in range(10):
   log.write(f'compiler={compiler} run={n+1} cases=21\n');log.flush()
   subprocess.run(['cargo','+'+compiler,'test','-p','penelope-sqlite','--test','tasks','task_columns'],env=dict(env,**extra),stdout=log,stderr=subprocess.STDOUT,check=True)
