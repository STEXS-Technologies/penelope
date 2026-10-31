from pathlib import Path
import os,subprocess
p=Path('crates/penelope-sqlite/src/process.rs');out=Path('target/commit-validation');original=p.read_bytes();s=original.decode();needle='decode::<ProcessOutcome>(bytes, checksum)?';assert s.count(needle)==1
changed=s.replace(needle,'decode::<ProcessOutcome>(bytes, Digest::of(bytes).0.as_slice())?')
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
try:
 p.write_text(changed)
 with (out/'control-checksum.log').open('w') as log:result=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','receipt_reconstruction_rejects_missing_and_substituted_constituents','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
 assert result.returncode==101
 log=(out/'control-checksum.log').read_text();assert 'test result: FAILED. 0 passed; 1 failed' in log and 'outcome/checksum' in log and 'left: Ok(' in log
finally:p.write_bytes(original)
with (out/'restored-test.log').open('w') as log:subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','receipt_reconstruction_rejects_missing_and_substituted_constituents'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
print('Bypassed outcome checksum validation was detected; restored receipt reconstruction test passes.')
