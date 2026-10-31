"""Deliberately disable each quota guard and require the gated race to catch it."""
from pathlib import Path
import os,subprocess
out=Path(__file__).resolve().parent;source=Path('crates/penelope-sqlite/src/process.rs');original=source.read_bytes()
pin='d45b988a9ae9b2e264aacc9ef81a8d7f1812c08d'
assert original==subprocess.check_output(['git','show',pin+':'+str(source)])
(out/'baseline-process.rs').write_bytes(original)
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
for label,guard,test,kind,case in [
 ('bytes','next > limits.max_scope_bytes','concurrent_ports_share_the_exact_process_byte_budget','bytes','payload-payload-cross-definition'),
 ('scopes','count >= limits.max_scopes','concurrent_new_processes_share_the_scope_admission_limit','scopes','payload-payload'),
]:
 try:
  text=original.decode();start=text.index('pub(crate) fn charge_process(');end=text.index('fn verify_committed_batch(',start)
  section=text[start:end];assert section.count('if '+guard+' {')==1
  section=section.replace('if '+guard+' {','if '+guard+' && false {')
  source.write_text(text[:start]+section+text[end:])
  with (out/f'control-{label}.log').open('w') as log:
   result=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process',test,'--','--nocapture','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT)
  text=(out/f'control-{label}.log').read_text();assert result.returncode==101
  assert f'kind={kind} case={case} admitted=2 accepted=2' in text
  assert 'test result: FAILED. 0 passed; 1 failed' in text
 finally:source.write_bytes(original)
with (out/'restored-process.log').open('w') as log:subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','--','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
assert 'test result: ok. 54 passed; 0 failed' in (out/'restored-process.log').read_text()
print('Both missing-guard controls admitted two writes; restored complete 54-test process suite passes.')
