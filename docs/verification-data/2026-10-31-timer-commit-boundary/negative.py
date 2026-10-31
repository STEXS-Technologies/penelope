from pathlib import Path
import os,subprocess,json
root=Path.cwd();out=Path(__file__).resolve().parent
source=root/'crates/penelope-sqlite/src/leases.rs';original=source.read_text()
env=os.environ|{'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','TMPDIR':str(root/'target/qualification-temp'),'RUST_TEST_THREADS':'1'}
results={}
try:
 for operation in ['schedule','cancel','claim_due','acknowledge','rollback','quota']:
  altered=original
  if operation=='rollback':
   altered=altered.replace('let transaction =','let mut transaction =')
   altered=altered.replace('transaction.commit()', 'transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore); transaction.commit()')
  else:
   method='schedule' if operation=='quota' else operation
   offset=altered.index('impl TimerScheduler' if method in ['schedule','cancel'] else 'impl TimerClaimStore')
   start=altered.index('    async fn '+method+'(',offset)
   end=altered.find('    async fn ',start+10)
   if end<0:end=len(altered)
   piece=altered[start:end]
   if operation=='quota':
    needle='.saturating_add(128)';assert piece.count(needle)==1
    piece=piece.replace(needle,'.saturating_add(129)')
   elif operation=='claim_due':
    needle='transaction.commit().map_err(|error| db_error(&error))?;';assert piece.count(needle)==1
    piece=piece.replace(needle,'let _ignored_commit = transaction.commit();')
   else:
    needle='transaction.commit().map_err(|error| db_error(&error))';assert piece.count(needle)==1
    piece=piece.replace(needle,'{ let _ignored_commit = transaction.commit(); Ok(()) }')
   altered=altered[:start]+piece+altered[end:]
  source.write_text(altered)
  result=subprocess.run(['cargo','test','-p','penelope-sqlite','--lib','deferred_timer_commit_failure','--','--nocapture'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  (out/('negative-'+operation+'.txt')).write_bytes(result.stdout)
  text=result.stdout.decode();assert result.returncode!=0 and 'test result: FAILED' in text
  marker='is_autocommit()' if operation=='rollback' else 'quota must charge exact timer bytes once' if operation=='quota' else 'left: Ok('
  assert marker in text
  results[operation]=result.returncode
  source.write_text(original)
finally:source.write_text(original)
(out/'negative-results.json').write_text(json.dumps(results,indent=2)+'\n')
print(results)
