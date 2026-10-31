from pathlib import Path
import os,subprocess,json,re,hashlib
cases=['head','record','blob']
env=dict(os.environ,TMPDIR='/home/ac/projects/penelope/target/qualification-temp')
rows=[]
for toolchain,log in [('stable','workspace'),('1.85.0','msrv')]:
 text=Path(f'target/sqlite-read-errors-{log}.log').read_text()
 binary=re.search(r'Running unittests src/lib.rs \(([^)]*penelope_sqlite[^)]+)\)',text)[1]
 digest=hashlib.sha256(Path(binary).read_bytes()).hexdigest()
 for iteration in range(10):
  for case in cases:
   command=[binary,'--exact',f'tasks::fault_tests::{case}_reads_preserve_engine_error_categories_and_recover','--nocapture','--test-threads=1']
   run=subprocess.run(command,env=env,capture_output=True,text=True,timeout=10)
   outcomes=re.findall(r'(task_\w+) SQLite code (\d+): (\w+), expected (\w+); repair/reopen passed',run.stdout)
   row=dict(toolchain=toolchain,iteration=iteration,case=case,command=command,binary_sha256=digest,returncode=run.returncode,stdout=run.stdout,stderr=run.stderr,outcomes=outcomes)
   rows.append(row);Path('target/sqlite-read-errors-repeated.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
   assert run.returncode==0 and 'test result: ok. 1 passed; 0 failed;' in run.stdout,row
   assert len(outcomes)==7 and all(actual==expected for table,code,actual,expected in outcomes),row
print('60 isolated matrices passed: 420 injected read failures, each repaired and reopened.')
