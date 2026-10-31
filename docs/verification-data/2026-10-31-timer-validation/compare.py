from pathlib import Path
import os,subprocess,shutil,json
root=Path.cwd();out=root/'target/timer-validation';source=root/'crates/penelope-sqlite/src/leases.rs';candidate=source.read_bytes();(out/'candidate-leases.rs').write_bytes(candidate)
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
try:
 for label,content in [('baseline',(out/'baseline-leases.rs').read_bytes()),('candidate',candidate)]:
  source.write_bytes(content)
  with (out/f'{label}-tests.log').open('w') as log:subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  with (out/f'{label}-build.log').open('w') as log:
   result=subprocess.run(['cargo','build','--release','-p','penelope-sqlite','--bench','lease_completion_allocations','--message-format=json'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
  artifacts=[json.loads(line) for line in result.stdout.splitlines()];binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')=='lease_completion_allocations');shutil.copy2(binary,out/f'{label}-probe')
  for repeat in range(4):
   with (out/f'{label}-{repeat}.jsonl').open('w') as log:subprocess.run([str(out/f'{label}-probe')],env=env,stdout=log,check=True)
  print(label,'tests/build/4 allocation probes passed',flush=True)
finally:source.write_bytes(candidate)
