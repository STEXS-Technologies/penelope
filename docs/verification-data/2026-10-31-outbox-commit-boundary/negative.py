from pathlib import Path
import os,subprocess,json
root=Path.cwd();out=Path(__file__).resolve().parent
source=root/'crates/penelope-sqlite/src/leases.rs';original=source.read_text()
env=os.environ|{'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','TMPDIR':str(root/'target/qualification-temp'),'RUST_TEST_THREADS':'1'}
results={}
try:
 for operation in ['claim','acknowledge','renew','rollback']:
  altered=original
  if operation=='rollback':
   altered=altered.replace('let transaction =','let mut transaction =')
   altered=altered.replace('transaction.commit()', 'transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore); transaction.commit()')
  else:
   start=altered.index('    async fn '+operation+'(')
   end=altered.find('    async fn ',start+10)
   if end<0:end=len(altered)
   piece=altered[start:end]
   if operation=='acknowledge':
    needle='transaction.commit().map_err(|error| db_error(&error))'
    assert piece.count(needle)==1
    piece=piece.replace(needle,'{ let _ignored_commit = transaction.commit(); Ok(()) }')
   else:
    needle='transaction.commit().map_err(|error| db_error(&error))?;'
    assert piece.count(needle)==1
    piece=piece.replace(needle,'let _ignored_commit = transaction.commit();')
   altered=altered[:start]+piece+altered[end:]
  source.write_text(altered)
  result=subprocess.run(['cargo','test','-p','penelope-sqlite','--lib','deferred_outbox_commit_failure','--','--nocapture'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  (out/('negative-'+operation+'.txt')).write_bytes(result.stdout)
  text=result.stdout.decode();assert result.returncode!=0 and 'test result: FAILED' in text
  assert ('left: Ok(' in text) if operation!='rollback' else ('assertion failed:' in text and 'is_autocommit()' in text)
  results[operation]=result.returncode
  source.write_text(original)
finally:source.write_text(original)
(out/'negative-results.json').write_text(json.dumps(results,indent=2)+'\n')
print(results)
