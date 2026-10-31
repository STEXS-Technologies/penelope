import pathlib,subprocess,os,json,hashlib
root=pathlib.Path.cwd();out=root/'target/task-replay-memo';src=root/'crates/penelope-sqlite/src/tasks.rs';candidate=src.read_bytes(); baseline=subprocess.check_output(['git','show','05cd3d0124b85d33a796281941850a8169cdbb98:crates/penelope-sqlite/src/tasks.rs'])
pre_guard=subprocess.check_output(['git','show','50856b001f81978daea5044947557a26429d9740:crates/penelope-sqlite/src/tasks.rs'])
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
benches=['task_blob_commits','task_reconstruction','task_reconstruction_allocations']
try:
 for variant,data in [('pre_guard',pre_guard),('baseline',baseline),('candidate',candidate)]:
  src.write_bytes(data);(out/(variant+'-tasks.rs')).write_bytes(data)
  command=['cargo','bench','-p','penelope-sqlite']
  for bench in benches:command+=['--bench',bench]
  with (out/(variant+'-build.log')).open('w') as log:subprocess.run(command+['--no-run'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  for bench in benches:
   binary=next(p for p in (root/'target/release/deps').glob(bench+'-*') if p.is_file() and p.suffix=='')
   target=out/(variant+'-'+bench);target.write_bytes(binary.read_bytes());target.chmod(0o755)
finally:src.write_bytes(candidate)
names=[v+'-'+b for v in ['pre_guard','baseline','candidate'] for b in benches]+[v+'-tasks.rs' for v in ['pre_guard','baseline','candidate']]
(out/'hashes.json').write_text(json.dumps({n:hashlib.sha256((out/n).read_bytes()).hexdigest() for n in names},indent=2)+'\n')
