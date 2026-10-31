from pathlib import Path
import os,subprocess,json,re,hashlib
cases=[f'cache_usage::{source}_cache_rejects_{direction}_usage_until_repaired' for source in ['committed','rebuilt'] for direction in ['lower','higher']]
rows=[]
env=dict(os.environ,TMPDIR='/home/ac/projects/penelope/target/qualification-temp')
for compiler,log in [('stable','workspace'),('1.85.0','msrv')]:
 text=Path(f'target/sqlite-cache-usage-{log}.log').read_text()
 binary=re.search(r'Running tests/tasks.rs \(([^)]+)\)',text)[1]
 digest=hashlib.sha256(Path(binary).read_bytes()).hexdigest()
 for iteration in range(25):
  for case in cases:
   command=[binary,'--exact',case,'--nocapture','--test-threads=1']
   run=subprocess.run(command,env=env,capture_output=True,text=True,timeout=10)
   row=dict(compiler=compiler,iteration=iteration,case=case,command=command,binary_sha256=digest,returncode=run.returncode,stdout=run.stdout,stderr=run.stderr)
   rows.append(row);Path('target/sqlite-cache-usage-repeated.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
   assert run.returncode==0 and 'test result: ok. 1 passed; 0 failed;' in run.stdout,row
print('200 isolated rejection, rollback, repair, retry and reopen scenarios passed across both compilers.')
