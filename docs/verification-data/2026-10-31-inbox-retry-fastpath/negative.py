from pathlib import Path
import hashlib,json,os,re,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent/'negative';out.mkdir(exist_ok=True)
path=root/'crates/penelope-sqlite/src/process.rs';original=path.read_text();parent='8c11d33030c38075421a03f8f17f513d6dfe0d3d';baseline=subprocess.check_output(['git','show',parent+':crates/penelope-sqlite/src/process.rs'],text=True)
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
a=original.index('fn existing_input_matches(');b=original.index('fn accept_encoded_input(',a);part=original[a:b]
assert part.count('if Digest::of(stored).0.as_slice() != checksum {')==1 and part.count('if decoded != *input {')==1
mutate=lambda edited:original[:a]+edited+original[b:]
checksum=mutate(part.replace('if Digest::of(stored).0.as_slice() != checksum {','if false {'))
semantic=mutate(part.replace('if decoded != *input {','if false {'))
scoped=mutate(part.replace('WHERE process_key=?1 AND input_id=?2','WHERE input_id=?2'))
recheck,count=re.subn(r'if existing_input_matches\(connection, input, key, bytes, limits\)\? \{\s*return Ok\(true\);\s*\}', '',original,count=1);assert count==1
validation='inbox_validation::duplicate_admission_validates_evidence_without_the_writer_lock';lock='inbox_retry_lock::pending_and_consumed_retries_preserve_quota_scope_and_committed_state';race='process::inbox_retry_tests::concurrent_admission_is_rechecked_before_quota_charge'
mutants=[('baseline-lock',baseline,['--test','process',validation],validation),('checksum',checksum,['--test','process',validation],validation),('semantic',semantic,['--test','process',validation],validation),('scope',scoped,['--test','process',lock],lock),('recheck',recheck,['--lib','concurrent_admission_is_rechecked_before_quota_charge'],race)]
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
r=subprocess.run(['cargo','test','--locked','-p','penelope-sqlite','--test','process','--','inbox_retry_lock','inbox_validation','--nocapture'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,check=True);(out/'restored.txt').write_text(r.stdout)
(out/'results.json').write_text(json.dumps(dict(results=results,restored_sha256=hashlib.sha256(path.read_bytes()).hexdigest()),indent=2)+'\n')
