from pathlib import Path
import os,subprocess
source=Path('crates/penelope-sqlite/src/inbox.rs');out=Path(__file__).resolve().parent;original=source.read_bytes();s=original.decode();env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
for label,needle,replacement,case in [('checksum','decode(bytes,checksum)?','decode(bytes,penelope_core::task::Digest::of(bytes).0.as_slice())?','checksum'),('bound','column_bytes(row,1,record_limit)','column_bytes(row,1,u32::MAX)','payload-size')]:
 assert s.count(needle)==1
 try:
  source.write_text(s.replace(needle,replacement))
  with (out/f'control-{label}.log').open('w') as log:result=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','discovery_rejects_corrupt_misindexed_and_oversized_inputs_without_consuming_them','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
  assert result.returncode==101
  log=(out/f'control-{label}.log').read_text();assert f'case={case} reopened=false' in log and 'left: Ok(' in log and 'test result: FAILED. 0 passed; 1 failed' in log
 finally:source.write_bytes(original)
 print(label,'missing protection detected',flush=True)
with (out/'restored-process.log').open('w') as log:subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
print('All restored process tests pass.',flush=True)
