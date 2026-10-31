from pathlib import Path
import os,subprocess
root=Path.cwd();source=root/'crates/penelope-sqlite/src/process.rs';original=source.read_bytes();out=root/'target/process-commit-boundary'
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
text=original.decode();needle='    transaction.commit().map_err(|error| db_error(&error))?;\n    Ok(receipt)';assert text.count(needle)==1
mutations={
 'negative-commit-ack':text.replace(needle,'    let _ignored = transaction.commit();\n    Ok(receipt)'),
 'negative-inbox-ack':text.replace('                transaction.commit().map_err(|error| db_error(&error))?;\n                Ok(InboxAcceptanceReceipt','                let _ignored = transaction.commit();\n                Ok(InboxAcceptanceReceipt'),
}
a=text.index('pub(crate) fn commit_process(');b=text.index('\n#[async_trait]',a);part=text[a:b];assert part.count('    let transaction = connection')==1
part=part.replace('    let transaction = connection','    let mut transaction = connection').replace(needle,'    transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);\n'+needle)
mutations['negative-rollback']=text[:a]+part+text[b:]
try:
 for name,changed in mutations.items():
  assert changed!=text
  source.write_text(changed)
  with (out/(name+'.txt')).open('w') as log:
   result=subprocess.run(['cargo','test','-p','penelope-sqlite','--all-features','--lib','commit_boundary_tests','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
  assert result.returncode!=0,name
  log=(out/(name+'.txt')).read_text();assert 'test result: FAILED' in log,name
  if name=='negative-rollback':assert 'connection.is_autocommit()' in log
  else:assert 'left: Ok(' in log and 'right: Err(Conflict)' in log
finally:source.write_bytes(original)
assert source.read_bytes()==original
print('All three negative controls failed as expected; production source restored.')
