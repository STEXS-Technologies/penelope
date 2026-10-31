from pathlib import Path
import os,subprocess,json,hashlib
root=Path.cwd();out=Path(__file__).resolve().parent;src=root/'crates/penelope-sqlite/src/worker.rs';original=src.read_bytes();names=[]
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
benches=['lease_claim','lease_batch']
try:
 for variant in ['baseline','candidate']:
  data=subprocess.check_output(['git','show','a0a7a6c69b04cca7a1056ab5eecf65df16db40f2:crates/penelope-sqlite/src/worker.rs']) if variant=='baseline' else original
  src.write_bytes(data);name=variant+'-worker.rs';(out/name).write_bytes(data);names.append(name)
  with (out/(variant+'-build.log')).open('w') as log:subprocess.run(['cargo','bench','-p','penelope-sqlite','--bench','lease_claim','--bench','lease_batch','--no-run','--message-format=json'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  artifacts={}
  for line in (out/(variant+'-build.log')).read_text().splitlines():
   try:r=json.loads(line)
   except json.JSONDecodeError:continue
   if r.get('reason')=='compiler-artifact' and r.get('executable') and r['target']['name'] in benches:artifacts[r['target']['name']]=r['executable']
  assert set(artifacts)==set(benches)
  for bench in benches:
   name=variant+'-'+bench;target=out/name;target.write_bytes(Path(artifacts[bench]).read_bytes());target.chmod(0o755);names.append(name)
finally:src.write_bytes(original)
for name,path in [('worker_panic_tests.rs','crates/penelope-sqlite/src/worker_panic_tests.rs')]+[(bench+'.rs','crates/penelope-sqlite/benches/'+bench+'.rs') for bench in benches]:
 (out/name).write_bytes(Path(path).read_bytes());names.append(name)
(out/'hashes.json').write_text(json.dumps({name:hashlib.sha256((out/name).read_bytes()).hexdigest() for name in names},indent=2)+'\n')
