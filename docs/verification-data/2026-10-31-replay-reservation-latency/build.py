from pathlib import Path
import os,subprocess,json,shutil
root=Path.cwd();out=Path(__file__).resolve().parent;source=root/'crates/penelope-sqlite/src/process.rs';original=source.read_bytes()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
try:
 for label in ['baseline','candidate']:
  source.write_bytes((out/f'{label}-process.txt').read_bytes())
  with (out/f'{label}-tests.txt').open('w') as log:
   subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','--all-features'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  with (out/f'{label}-build.txt').open('w') as log:
   result=subprocess.run(['cargo','build','--release','-p','penelope-sqlite','--bench','process_replay','--message-format=json'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
  artifacts=[json.loads(line) for line in result.stdout.splitlines()]
  binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')=='process_replay')
  shutil.copy2(binary,out/f'{label}-process_replay')
  print(label,'tests and build passed',flush=True)
finally: source.write_bytes(original)
