from pathlib import Path
import os,subprocess,shutil,json
root=Path.cwd();out=root/'target/pending-validation';source=root/'crates/penelope-sqlite/src/inbox.rs';candidate=source.read_bytes();(out/'candidate-inbox.rs').write_bytes(candidate)
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
try:
 for label,content in [('baseline',(out/'baseline-inbox.rs').read_bytes()),('candidate',candidate)]:
  source.write_bytes(content)
  with (out/f'{label}-tests.log').open('w') as log:subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  for bench in ['pending_input_allocations','pending_inputs']:
   with (out/f'{label}-{bench}-build.log').open('w') as log:
    result=subprocess.run(['cargo','build','--release','-p','penelope-sqlite','--bench',bench,'--message-format=json'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
   artifacts=[json.loads(line) for line in result.stdout.splitlines()];binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==bench);shutil.copy2(binary,out/f'{label}-{bench}')
  for repeat in range(4):
   with (out/f'{label}-{repeat}.jsonl').open('w') as log:subprocess.run([str(out/f'{label}-pending_input_allocations')],env=env,stdout=log,check=True)
  print(label,'tests/build/4 allocation probes passed',flush=True)
finally:source.write_bytes(candidate)
