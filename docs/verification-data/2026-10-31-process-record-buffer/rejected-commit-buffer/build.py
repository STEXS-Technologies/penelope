from pathlib import Path
import hashlib,json,os,shutil,subprocess
root=Path.cwd();out=root/'target/process-encoding-reuse';source=root/'crates/penelope-sqlite/src/process.rs';original=source.read_bytes();parent=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert not (out/'metadata.json').exists()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1')
metadata={'parent':parent,'sources':{},'binaries':{},'rustc':subprocess.check_output(['rustc','-Vv'],text=True)}
try:
 for variant in ['baseline','candidate']:
  source.write_bytes(subprocess.check_output(['git','show',parent+':crates/penelope-sqlite/src/process.rs']) if variant=='baseline' else original)
  metadata['sources'][variant]=hashlib.sha256(source.read_bytes()).hexdigest()
  with (out/(variant+'-tests.txt')).open('w') as log:
   subprocess.run(['cargo','test','-p','penelope-sqlite','--all-features','--test','process','commit_encoding','--','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  with (out/(variant+'-build.txt')).open('w') as log:
   result=subprocess.run(['cargo','build','--release','-p','penelope-sqlite','--bench','process_encoding','--bench','process_encoding_allocations','--message-format=json'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
  artifacts=[json.loads(line) for line in result.stdout.splitlines()]
  for name in ['process_encoding','process_encoding_allocations']:
   path=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==name)
   dest=out/(variant+'-'+name);shutil.copy2(path,dest);metadata['binaries'][dest.name]=hashlib.sha256(dest.read_bytes()).hexdigest()
finally:source.write_bytes(original)
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
