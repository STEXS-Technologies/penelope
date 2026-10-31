import pathlib,subprocess,os,json,hashlib
root=pathlib.Path.cwd();out=root/'target/task-cache-revision';src=root/'crates/penelope-sqlite/src/tasks.rs';candidate=src.read_bytes();baseline=subprocess.check_output(['git','show','50856b001f81978daea5044947557a26429d9740:crates/penelope-sqlite/src/tasks.rs'])
old=b'''    connection
        .prepare_cached("PRAGMA main.data_version")
        .map_err(|error| sql_error(&error))?
        .query_row([], |row| row.get(0))'''
new=b'''    connection
        .query_row("PRAGMA main.data_version", [], |row| row.get(0))'''
assert old in candidate;uncached=candidate.replace(old,new)
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
try:
 for name,data in [('baseline',baseline),('uncached',uncached),('cached',candidate)]:
  src.write_bytes(data);(out/(name+'-tasks.rs')).write_bytes(data)
  with (out/(name+'-build.log')).open('w') as log:subprocess.run(['cargo','bench','-p','penelope-sqlite','--bench','task_blob_commits','--no-run'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  binary=next(p for p in (root/'target/release/deps').glob('task_blob_commits-*') if p.is_file() and p.suffix=='')
  target=out/name;target.write_bytes(binary.read_bytes());target.chmod(0o755)
finally:src.write_bytes(candidate)
(out/'hashes.json').write_text(json.dumps({n:hashlib.sha256((out/n).read_bytes()).hexdigest() for n in ['baseline','uncached','cached','baseline-tasks.rs','uncached-tasks.rs','cached-tasks.rs']},indent=2)+'\n')
