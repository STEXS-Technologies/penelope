import subprocess,os,pathlib
out=pathlib.Path(__file__).resolve().parent
for compiler in ['stable','1.85.0']:
 env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(pathlib.Path.cwd()/'target/qualification-temp'))
 if compiler!='stable':env['CARGO_TARGET_DIR']='target/worker-audit-msrv'
 for repeat in range(10):
  with (out/f'repeat-{compiler}-{repeat:02}.log').open('w') as log:
   subprocess.run(['cargo','+'+compiler,'test','-p','penelope-sqlite','--test','tasks','task_cache_revision::','--','--test-threads=1','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
 print(compiler,'10 passes',flush=True)
