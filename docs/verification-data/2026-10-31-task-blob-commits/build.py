import pathlib,subprocess,os,hashlib,json
root=pathlib.Path.cwd(); out=root/'target/task-blob-commit-comparison'; src=root/'crates/penelope-sqlite/src/tasks.rs'; candidate=src.read_bytes(); base=subprocess.check_output(['git','show','9a1e80338b581600691f41ef6a7cb39e4f2255c9:crates/penelope-sqlite/src/tasks.rs']); env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
try:
 for name,data in [('baseline',base),('candidate',candidate)]:
  src.write_bytes(data); (out/(name+'-tasks.rs')).write_bytes(data)
  with (out/(name+'-build.log')).open('w') as log: subprocess.run(['cargo','bench','-p','penelope-sqlite','--bench','task_blob_commits','--no-run'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  binaries=list((root/'target/release/deps').glob('task_blob_commits-*')); binary=next(p for p in binaries if p.is_file() and p.suffix=='')
  target=out/name;target.write_bytes(binary.read_bytes());target.chmod(0o755)
finally: src.write_bytes(candidate)
(out/'hashes.json').write_text(json.dumps({n:hashlib.sha256((out/n).read_bytes()).hexdigest() for n in ['baseline','candidate','baseline-tasks.rs','candidate-tasks.rs']},indent=2)+'\n')
