from pathlib import Path
import os,subprocess,json
out=Path(__file__).resolve().parent;runs=[]
for toolchain in ['stable','1.85.0']:
 for repeat in range(10):
  env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path('target/qualification-temp').resolve()))
  if toolchain=='1.85.0': env['CARGO_TARGET_DIR']='target/worker-audit-msrv'
  result=subprocess.run(['cargo',f'+{toolchain}','test','-p','penelope-sqlite','--test','tasks','--all-features','contending_reservations','--','--nocapture'],env=env,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  name=f'repeat-{toolchain}-{repeat}.txt';(out/name).write_text(result.stdout)
  assert result.returncode==0 and '1 passed; 0 failed' in result.stdout and result.stdout.count('delayed_start_rejected=true')==24
  runs.append({'toolchain':toolchain,'repeat':repeat,'exit':result.returncode,'log':name})
(out/'repeats.json').write_text(json.dumps(runs,indent=2)+'\n');print('20 isolated executions passed: 480 contention histories and 960 competing requests.')
