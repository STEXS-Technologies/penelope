from pathlib import Path
import os,subprocess
root=Path.cwd();source=root/'crates/penelope-sqlite/src/lifecycle.rs';original=source.read_bytes();text=original.decode();out=root/'target/task-retirement-boundary'
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
needle='        transaction.commit().map_err(|error| sql_error(&error))?;';assert text.count(needle)==1
variants={
 'negative-ack':text.replace(needle,'        let _ignored = transaction.commit();'),
 'negative-rollback':text.replace('        let transaction = self','        let mut transaction = self').replace(needle,'        transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);\n'+needle),
 'negative-scope':text.replace('DELETE FROM {table} WHERE scope=?1','DELETE FROM {table} WHERE scope=?1 OR 1=1'),
}
try:
 for name,changed in variants.items():
  assert changed!=text;source.write_text(changed)
  with (out/(name+'.txt')).open('w') as log:
   result=subprocess.run(['cargo','test','-p','penelope-sqlite','--all-features','--lib','deferred_retirement_failure','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
  assert result.returncode!=0
  log=(out/(name+'.txt')).read_text();assert 'test result: FAILED' in log and '1 failed' in log
  marker={'negative-ack':'left: Ok(','negative-rollback':'journal.connection.is_autocommit()','negative-scope':'all target history must be deleted inside the failing transaction'}[name]
  assert marker in log,(name,log)
finally:source.write_bytes(original)
assert source.read_bytes()==original
print('False ACK, missing rollback, and unscoped deletion all rejected; source restored.')
