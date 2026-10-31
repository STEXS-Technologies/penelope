from pathlib import Path
import os,subprocess
p=Path('crates/penelope-sqlite/src/process.rs');out=Path('target/inbox-validation');original=p.read_bytes();s=original.decode();needle='if Digest::of(stored).0.as_slice() != checksum {';assert s.count(needle)==1
changed=s.replace(needle,'if false {')
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
try:
 p.write_text(changed)
 with (out/'control-checksum.log').open('w') as log:result=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','duplicate_admission_preserves_encodings_conflicts_and_corruption_rejection','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
 assert result.returncode==101
 log=(out/'control-checksum.log').read_text();assert 'test result: FAILED. 0 passed; 1 failed' in log and 'consumed=false case=checksum reopened=false' in log and 'left: Ok(' in log
finally:p.write_bytes(original)
with (out/'restored-test.log').open('w') as log:subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','inbox_validation::'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
print('Bypassed fast-path checksum was detected; both restored inbox validation tests pass.')
