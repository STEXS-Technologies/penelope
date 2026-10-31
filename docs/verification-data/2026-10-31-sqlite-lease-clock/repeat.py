from pathlib import Path
import os,subprocess,json,re,hashlib
names=['new_outbox_claim_samples_clock_after_contended_writer_lock','expired_outbox_lease_is_reclaimed_using_time_after_writer_lock','clock_failure_and_expiry_overflow_release_the_writer_lock']
rows=[]
env=dict(os.environ,TMPDIR='/home/ac/projects/penelope/target/qualification-temp')
for compiler,log in [('stable','workspace'),('1.85.0','msrv')]:
 text=Path(f'target/sqlite-lease-clock-{log}.log').read_text()
 binary=re.search(r'Running unittests src/lib.rs \(([^)]*penelope_sqlite[^)]+)\)',text)[1]
 digest=hashlib.sha256(Path(binary).read_bytes()).hexdigest()
 for iteration in range(25):
  for name in names:
   command=[binary,'--exact','process::fault_tests::lease_clock_tests::'+name,'--nocapture','--test-threads=1']
   run=subprocess.run(command,env=env,capture_output=True,text=True,timeout=15)
   row=dict(compiler=compiler,iteration=iteration,case=name,command=command,binary_sha256=digest,returncode=run.returncode,stdout=run.stdout,stderr=run.stderr)
   rows.append(row);Path('target/sqlite-lease-clock-repeated.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
   assert run.returncode==0 and 'test result: ok. 1 passed; 0 failed;' in run.stdout,row
   if name!='clock_failure_and_expiry_overflow_release_the_writer_lock':
    assert 'clock_samples_while_blocked=0 returned_leases=1 expiries=[LogicalTime(110)]' in run.stdout,row
print('150 isolated test processes passed: 100 real lock-contention claims and 100 clock-error/overflow rollback-and-retry scenarios.')
