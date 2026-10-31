from pathlib import Path
import os,subprocess
root=Path.cwd();source=root/'crates/penelope-sqlite/src/tasks.rs';original=source.read_bytes();text=original.decode();out=root/'target/task-commit-boundary'
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
needle='        transaction.commit().map_err(|error| sql_error(&error))?;\n        self.cache = Some((projection, digest, used, version));';assert text.count(needle)==1
variants={
 'negative-ack':text.replace(needle,'        let _ignored = transaction.commit();\n        self.cache = Some((projection, digest, used, version));'),
 'negative-cache':text.replace(needle,'        self.cache = Some((projection, digest, used, version));\n        transaction.commit().map_err(|error| sql_error(&error))?;'),
}
a=text.index('    fn commit(');b=text.index('    fn read_blob(',a);part=text[a:b];assert part.count('        let transaction = self')==1
part=part.replace('        let transaction = self','        let mut transaction = self').replace(needle,'        transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);\n'+needle)
variants['negative-rollback']=text[:a]+part+text[b:]
try:
 for name,changed in variants.items():
  assert changed!=text;source.write_text(changed)
  with (out/(name+'.txt')).open('w') as log:
   result=subprocess.run(['cargo','test','-p','penelope-sqlite','--all-features','--lib','tasks::commit_boundary_tests','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
  assert result.returncode!=0
  log=(out/(name+'.txt')).read_text();assert 'test result: FAILED' in log and '2 failed' in log
  marker={'negative-ack':'left: Ok(','negative-cache':'a failed COMMIT must not publish a new cached frontier','negative-rollback':'journal.connection.is_autocommit()'}[name]
  assert marker in log,(name,log)
finally:source.write_bytes(original)
assert source.read_bytes()==original
print('False ACK, premature cache publication, and missing rollback all rejected; source restored.')
