from pathlib import Path
import os,subprocess
p=Path('crates/penelope-sqlite/src/leases.rs');out=Path('target/timer-validation');original=p.read_bytes();s=original.decode();needle='    decode(bytes, checksum).map(Some)';assert s.count(needle)==1
changed=s.replace(needle,'    let _ = checksum;\n    decode(bytes, Digest::of(bytes).0.as_slice()).map(Some)')
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
try:
 p.write_text(changed)
 with (out/'control-checksum.log').open('w') as log:result=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','timer_validation::','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
 assert result.returncode==101
 log=(out/'control-checksum.log').read_text();assert 'test result: FAILED. 0 passed; 1 failed' in log and 'action_row=false fault=checksum' in log and 'left: Ok(())' in log
finally:p.write_bytes(original)
with (out/'restored-test.log').open('w') as log:subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','timer_validation::'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
print('Bypassed checksum validation was detected; restored timer validation test passes.')
