from pathlib import Path
import os,subprocess
source=Path('crates/penelope-sqlite/src/process.rs');out=Path(__file__).resolve().parent;original=source.read_bytes();s=original.decode();env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(Path.cwd()/'target/qualification-temp'))
controls=[('consumption','if consumed {','if consumed && false {','consumed-input-at-next-frontier'),('source','if let Some(source) = &commit.canonical_source_event_id {','if let Some(source) = commit.canonical_source_event_id.as_ref().filter(|_| input.input_id.as_str() != "inp_competing") {','reused-source-at-next-frontier')]
for label,needle,replacement,case in controls:
 assert needle in s
 try:
  source.write_text(s.replace(needle,replacement,1))
  with (out/f'control-{label}.log').open('w') as log:result=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','next_frontier_rejects_consumed_inputs_and_reused_sources_during_admission','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
  assert result.returncode==101
  log=(out/f'control-{label}.log').read_text();assert f'case={case} admitted=2 accepted=2' in log and 'test result: FAILED. 0 passed; 1 failed' in log
 finally:source.write_bytes(original)
 print(label,'missing guard detected in gated race',flush=True)
with (out/'restored-process.log').open('w') as log:subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
print('All restored process tests pass.',flush=True)
