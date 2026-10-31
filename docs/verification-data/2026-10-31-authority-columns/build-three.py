from pathlib import Path
import os,subprocess,shutil,hashlib,json
root=Path.cwd();out=root/'target/authority-columns-comparison';sources={name:root/'crates/penelope-sqlite/src'/f'{name}.rs' for name in ['tasks','authority']};final={name:p.read_bytes() for name,p in sources.items()}
for name,content in final.items():(out/f'cached-{name}.rs').write_bytes(content)
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'));hashes={}
try:
 for variant in ['baseline','uncached','cached']:
  for name,p in sources.items():p.write_bytes((out/f'{variant}-{name}.rs').read_bytes())
  with (out/(variant+'-final-build.log')).open('w') as log:
   subprocess.run(['cargo','build','--release','-p','penelope-sqlite','--bench','task_reconstruction','--bench','task_reconstruction_allocations'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  for bench,label in [('task_reconstruction','latency'),('task_reconstruction_allocations','allocations')]:
   files=[p for p in (root/'target/release/deps').glob(bench+'-*') if p.suffix=='' and os.access(p,os.X_OK)];assert len(files)==1
   dest=out/(variant+'-'+label);shutil.copy2(files[0],dest);hashes[dest.name]=hashlib.sha256(dest.read_bytes()).hexdigest()
finally:
 for name,p in sources.items():p.write_bytes(final[name])
(out/'final-binary-hashes.json').write_text(json.dumps(hashes,indent=2)+'\n')
