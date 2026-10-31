from pathlib import Path
import json,os,re,subprocess,sys
root=Path.cwd();out=root/'target/task-retirement-boundary';toolchain=sys.argv[1];assert toolchain in ['stable','msrv']
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
command=['cargo']
if toolchain=='msrv':env['CARGO_TARGET_DIR']='target/worker-audit-msrv';command+=['+1.85.0']
command+=['test','-p','penelope-sqlite','--all-features','--lib','deferred_retirement_failure','--','--nocapture']
records=[]
with (out/(toolchain+'-repetitions.txt')).open('w') as combined:
 for index in range(20):
  result=subprocess.run(command,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
  combined.write(f'REPETITION {index}\n'+result.stdout);combined.flush()
  assert result.returncode==0,(index,result.stdout)
  counts={name:int(re.search(name+r'=(\d+)',result.stdout)[1]) for name in ['deferred_retirement_cases']}
  assert counts=={'deferred_retirement_cases':12}
  records.append({'index':index,'exit_code':result.returncode,'counts':counts})
(out/(toolchain+'-repetitions.json')).write_text(json.dumps({'command':command,'repetitions':records},indent=2)+'\n')
print(toolchain,'20 repetitions, 240 retirement COMMIT-failure cases passed')
