from pathlib import Path
import os,subprocess,json,shutil
root=Path.cwd();out=Path(__file__).resolve().parent;source=root/'crates/penelope-sqlite/src/tasks.rs';candidate=(out/'candidate-tasks.rs').read_bytes()
manifest=root/'crates/penelope-sqlite/Cargo.toml';original_manifest=manifest.read_bytes()
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1');env.pop('PENELOPE_BENCH_EXTERNAL_COMMIT',None)
try:
 probe_path=os.path.relpath(out/'allocation-probe.rs',manifest.parent)
 manifest.write_text(original_manifest.decode()+'\n[[bench]]\nname = "task_commit_allocation_probe"\nharness = false\npath = '+json.dumps(probe_path)+'\n')
 for label,content in [('baseline',(out/'baseline-tasks.rs').read_bytes()),('candidate',candidate)]:
  source.write_bytes(content)
  with (out/f'{label}-tests.txt').open('w') as log: subprocess.run(['cargo','test','-p','penelope-sqlite','--test','tasks','--all-features'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  for bench in ['task_blob_commits','task_commit_allocation_probe']:
   with (out/f'{label}-{bench}-build.txt').open('w') as log:
    result=subprocess.run(['cargo','build','--release','-p','penelope-sqlite','--bench',bench,'--message-format=json'],env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
   artifacts=[json.loads(line) for line in result.stdout.splitlines()];binary=next(Path(a['executable']) for a in artifacts if a.get('executable') and a.get('target',{}).get('name')==bench);shutil.copy2(binary,out/f'{label}-{bench}')
  for repeat in range(4):
   with (out/f'{label}-alloc-{repeat}.jsonl').open('w') as log: subprocess.run([str(out/f'{label}-task_commit_allocation_probe')],env=env,stdout=log,check=True)
  print(label,'tests/build/allocation passed',flush=True)
finally:
 source.write_bytes(candidate)
 manifest.write_bytes(original_manifest)
