from pathlib import Path
import json,os,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent
path=root/'crates/penelope-executor/src/timer_input.rs';original=path.read_bytes()
legacy=(root/'crates/penelope-executor/tests/support/timer_input_legacy.rs').read_bytes();old=original[:original.index(b'/// Stable timer inbox input')]+legacy[legacy.index(b'/// Stable timer inbox input'):]
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
results=[]
try:
 for name,source,test,marker in [
  ('budget',old,'timer_input_allocations','timer input allocation budget'),
  ('metadata',original.replace(b'ContentDigest::sha256(&key)',b'if lease.owner.as_str() == "pri_other" { key.extend_from_slice(lease.owner.as_str().as_bytes()); }\n        ContentDigest::sha256(&key)'), 'timer_input_identity','assertion `left == right` failed'),
  ('prefix',original.replace(b'penelope.timer-input.v1',b'penelope.timer-input.v2'),'timer_input_identity','assertion `left == right` failed')]:
  assert source!=original;path.write_bytes(source)
  with (out/('negative-'+name+'.txt')).open('w') as log:r=subprocess.run(['cargo','test','-p','penelope-executor','--test',test,'--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
  assert r.returncode!=0 and marker in (out/('negative-'+name+'.txt')).read_text()
  results.append({'name':name,'rejected':True,'marker':marker})
finally:path.write_bytes(original)
assert path.read_bytes()==original
(out/'negative-results.json').write_text(json.dumps(results,indent=2)+'\n')
