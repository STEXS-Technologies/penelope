from pathlib import Path
import os,subprocess,json
root=Path.cwd();out=Path(__file__).resolve().parent
source=root/'crates/penelope-sqlite/src/leases.rs';original=source.read_bytes();parent=json.loads((out/'metadata.json').read_text())['parent']
env=os.environ|{'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','TMPDIR':str(root/'target/qualification-temp'),'RUST_TEST_THREADS':'1'}
results={}
try:
 source.write_bytes(subprocess.check_output(['git','show',parent+':crates/penelope-sqlite/src/leases.rs']))
 for name in ['outbox','timer']:
  result=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','lease_claim_profile_allocations','--',name],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  (out/('negative-'+name+'.txt')).write_bytes(result.stdout)
  text=result.stdout.decode();assert result.returncode!=0 and name+': allocations=' in text
  results[name]=result.returncode
 source.write_bytes(original)
 for field in ['tenant_id','process_id','definition_id','definition_version','definition_digest']:
  text=original.decode();needle='action.'+field+' == scope.'+field;assert text.count(needle)==1
  source.write_text(text.replace(needle,'true'))
  result=subprocess.run(['cargo','test','-p','penelope-sqlite','--lib','borrowed_claim_scope','--','--nocapture'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  (out/('negative-'+field+'.txt')).write_bytes(result.stdout)
  assert result.returncode!=0 and b'test result: FAILED' in result.stdout and b'left: true' in result.stdout
  results[field]=result.returncode
  source.write_bytes(original)
finally:source.write_bytes(original)
(out/'negative-results.json').write_text(json.dumps(results,indent=2)+'\n')
print(results)
