from pathlib import Path
import os,subprocess
out=Path(__file__).resolve().parent
for compiler in ['stable','1.85.0']:
 env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
 if compiler=='1.85.0':env['CARGO_TARGET_DIR']='target/worker-audit-msrv'
 for repeat in range(10):
  with (out/f'repeat-{compiler}-{repeat:02}.log').open('w') as log:subprocess.run(['cargo','+'+compiler,'test','-p','penelope-sqlite','--test','process','concurrent_quota::','--','--nocapture','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
 print(compiler,'10 concurrent-quota processes passed',flush=True)
