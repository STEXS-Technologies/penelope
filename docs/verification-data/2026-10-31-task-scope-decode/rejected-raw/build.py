import pathlib,subprocess,os,json,hashlib
root=pathlib.Path.cwd();out=root/'target/task-scope-decode';paths=['crates/penelope-sqlite/src/tasks.rs','crates/penelope-sqlite/src/lib.rs','crates/penelope-sqlite/Cargo.toml'];original={p:(root/p).read_bytes() for p in paths};pin='bd187069a30b0c3113089c83260c4b961db45ebe';benches=['task_blob_commits','task_reconstruction','task_reconstruction_allocations'];names=[]
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
try:
 for variant in ['baseline','candidate']:
  for path in paths:
   data=subprocess.check_output(['git','show',pin+':'+path]) if variant=='baseline' else original[path]
   (root/path).write_bytes(data);name=variant+'-'+pathlib.Path(path).name;(out/name).write_bytes(data);names.append(name)
  command=['cargo','bench','-p','penelope-sqlite']
  for bench in benches:command+=['--bench',bench]
  with (out/(variant+'-build.log')).open('w') as log:subprocess.run(command+['--no-run','--message-format=json'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  artifacts={}
  for line in (out/(variant+'-build.log')).read_text().splitlines():
   try:message=json.loads(line)
   except json.JSONDecodeError:continue
   if message.get('reason')=='compiler-artifact' and message.get('executable') and message['target']['name'] in benches:artifacts[message['target']['name']]=message['executable']
  assert set(artifacts)==set(benches)
  for bench in benches:
   binary=pathlib.Path(artifacts[bench])
   name=variant+'-'+bench;target=out/name;target.write_bytes(binary.read_bytes());target.chmod(0o755);names.append(name)
finally:
 for path,data in original.items():(root/path).write_bytes(data)
(out/'hashes.json').write_text(json.dumps({name:hashlib.sha256((out/name).read_bytes()).hexdigest() for name in names},indent=2)+'\n')

hashes=json.loads((out/'hashes.json').read_text())
assert hashes['baseline-task_reconstruction']!=hashes['candidate-task_reconstruction']
