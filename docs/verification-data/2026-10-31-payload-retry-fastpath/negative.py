from pathlib import Path
import hashlib,json,os,re,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent/'negative';out.mkdir(exist_ok=True)
path=root/'crates/penelope-sqlite/src/payloads.rs';original=path.read_text();parent='704688d5ec28eca0d277165e8a365d7d52edab3f';baseline=subprocess.check_output(['git','show',parent+':crates/penelope-sqlite/src/payloads.rs'],text=True)
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
assert original.count('column_bytes(row, 0, limit).map(|stored| stored == bytes)')==1
start=original.index('fn stored_payload_matches(');scoped=original[:start]+original[start:].replace('WHERE scope=?1 AND digest=?2','WHERE digest=?2')
recheck,count=re.subn(r'let stored = stored_payload_matches\([\s\S]*?\)\?;', 'let stored: Option<bool> = None;',original,count=1);assert count==1
mutants=[('baseline-lock',baseline,['--test','process','payload_retry_lock::exact_payload_retries_do_not_require_the_writer_lock'],'payload_retry_lock::exact_payload_retries_do_not_require_the_writer_lock'),('byte-equality',original.replace('column_bytes(row, 0, limit).map(|stored| stored == bytes)','column_bytes(row, 0, limit).map(|_stored| true)'),['--test','process','payload_retry_lock::locked_payload_retries_revalidate_committed_corruption'],'payload_retry_lock::locked_payload_retries_revalidate_committed_corruption'),('scope',scoped,['--test','process','payload_retry_lock::exact_payload_retries_do_not_require_the_writer_lock'],'payload_retry_lock::exact_payload_retries_do_not_require_the_writer_lock'),('recheck',recheck,['--lib','insertion_after_read_is_rechecked_before_quota_charge'],'payloads::tests::insertion_after_read_is_rechecked_before_quota_charge')]
results=[]
try:
 for name,source,args,marker in mutants:
  path.write_text(source)
  r=subprocess.run(['cargo','test','--locked','-p','penelope-sqlite',*args,'--','--nocapture'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
  (out/(name+'.txt')).write_text(r.stdout)
  assert r.returncode and 'test result: FAILED' in r.stdout and marker in r.stdout,(name,r.stdout)
  results.append(dict(name=name,rejected=True,marker=marker))
finally:path.write_text(original)
assert path.read_text()==original
r=subprocess.run(['cargo','test','--locked','-p','penelope-sqlite','--test','process','payload_retry_lock','--','--nocapture'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,check=True);(out/'restored.txt').write_text(r.stdout)
(out/'results.json').write_text(json.dumps(dict(results=results,restored_sha256=hashlib.sha256(path.read_bytes()).hexdigest()),indent=2)+'\n')
