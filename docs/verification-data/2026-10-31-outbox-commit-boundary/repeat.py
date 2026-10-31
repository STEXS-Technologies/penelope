from pathlib import Path
import os,subprocess,re,json
out=Path(__file__).resolve().parent;root=Path.cwd()
env=os.environ|{'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','TMPDIR':str(root/'target/qualification-temp'),'RUST_TEST_THREADS':'1'}
for toolchain in ['stable','msrv']:
 command=['cargo']
 local=env.copy()
 if toolchain=='msrv':
  command+=['+1.85.0'];local['CARGO_TARGET_DIR']=str(root/'target/worker-audit-msrv')
 command+=['test','-p','penelope-sqlite','--lib','deferred_outbox_commit_failure','--','--nocapture']
 rows=[]
 with (out/(toolchain+'-repetitions.txt')).open('wb') as log:
  for index in range(20):
   result=subprocess.run(command,env=local,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
   log.write(('REPETITION '+str(index)+'\n').encode()+result.stdout);log.flush()
   counts=list(map(int,re.findall(r'deferred_outbox_cases=(\d+)',result.stdout.decode())))
   assert result.returncode==0 and counts==[12]
   rows.append({'index':index,'exit_code':result.returncode,'cases':counts[0]})
 (out/(toolchain+'-repetitions.json')).write_text(json.dumps(rows,indent=2)+'\n')
 print(toolchain,sum(r['cases'] for r in rows),flush=True)
