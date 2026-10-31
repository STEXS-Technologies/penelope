"""Rebuild the historical control; restore current sources even if Cargo fails."""
import pathlib,subprocess,os,json,hashlib
root=pathlib.Path.cwd(); out=root/'target/task-scope-decode'; pin='50856b001f81978daea5044947557a26429d9740'
paths=['crates/penelope-sqlite/src/tasks.rs','crates/penelope-sqlite/src/lib.rs','crates/penelope-sqlite/Cargo.toml']; original={p:(root/p).read_bytes() for p in paths}
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'))
try:
 for path in paths:
  data=subprocess.check_output(['git','show',pin+':'+path]);(root/path).write_bytes(data);(out/('pre_guard-'+pathlib.Path(path).name)).write_bytes(data)
 with (out/'pre_guard-build.log').open('w') as log:subprocess.run(['cargo','bench','-p','penelope-sqlite','--bench','task_blob_commits','--no-run','--message-format=json'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
 binaries=[]
 for line in (out/'pre_guard-build.log').read_text().splitlines():
  try:r=json.loads(line)
  except json.JSONDecodeError:continue
  if r.get('reason')=='compiler-artifact' and r.get('executable') and r['target']['name']=='task_blob_commits':binaries.append(r['executable'])
 assert len(binaries)==1
 data=pathlib.Path(binaries[0]).read_bytes()
 expected=json.loads((out/'confirm-provenance.json').read_text())['sha256']['pre_guard']
 # Exact recorded identity is expected only in the original compiler/environment.
 (out/'rebuild-control.json').write_text(json.dumps(dict(sha256=hashlib.sha256(data).hexdigest(),recorded_sha256=expected,identical=hashlib.sha256(data).hexdigest()==expected),indent=2)+'\n')
 (out/'pre_guard').write_bytes(data);(out/'pre_guard').chmod(0o755)
finally:
 for path,data in original.items():(root/path).write_bytes(data)
for variant in ['baseline','candidate']:
 target=out/variant;target.write_bytes((out/(variant+'-task_blob_commits')).read_bytes());target.chmod(0o755)
print('Confirmation executables prepared; production sources restored.')
