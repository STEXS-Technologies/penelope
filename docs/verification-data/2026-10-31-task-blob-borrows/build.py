from pathlib import Path
import os,subprocess,shutil,json,hashlib
root=Path.cwd();out=root/'target/task-blob-comparison';source=root/'crates/penelope-sqlite/src/tasks.rs'
candidate=source.read_bytes();(out/'candidate-tasks.rs').write_bytes(candidate)
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
hashes={}
try:
 for variant,content in [('baseline',(out/'baseline-tasks.rs').read_bytes()),('candidate',candidate)]:
  source.write_bytes(content)
  with (out/(variant+'-build.log')).open('w') as log:
   subprocess.run(['cargo','build','--release','-p','penelope-sqlite','--bench','task_reconstruction','--bench','task_reconstruction_allocations','--bench','task_blobs','--bench','task_blob_allocations'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  for bench,label in [('task_reconstruction','reconstruction-latency'),('task_reconstruction_allocations','reconstruction-allocations'),('task_blobs','blob-latency'),('task_blob_allocations','blob-allocations')]:
   binaries=[p for p in (root/'target/release/deps').glob(bench+'-*') if p.suffix=='' and os.access(p,os.X_OK)]
   assert len(binaries)==1,binaries
   dest=out/(variant+'-'+label);shutil.copy2(binaries[0],dest);hashes[dest.name]=hashlib.sha256(dest.read_bytes()).hexdigest()
finally:source.write_bytes(candidate)
(out/'binary-hashes.json').write_text(json.dumps(hashes,indent=2)+'\n')
