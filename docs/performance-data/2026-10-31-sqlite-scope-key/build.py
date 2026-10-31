from pathlib import Path
import subprocess,os,json,hashlib,shutil
p=Path('crates/penelope-sqlite/src/tasks.rs');candidate=p.read_bytes()
baseline=subprocess.check_output(['git','show','7acf97f:crates/penelope-sqlite/src/tasks.rs'])
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR='/home/ac/projects/penelope/target/qualification-temp')
metadata={}
try:
 for label,contents in [('baseline',baseline),('candidate',candidate)]:
  p.write_bytes(contents)
  with open(f'target/sqlite-scope-latency-build-{label}.log','w') as log:
   command=['cargo','bench','-p','penelope-process-runtime','--bench','fence','--bench','dispatch','--no-run','--message-format=json']
   run=subprocess.run(command,env=env,stdout=log,stderr=subprocess.STDOUT)
  run.check_returncode()
  for line in Path(f'target/sqlite-scope-latency-build-{label}.log').read_text().splitlines():
   try: item=json.loads(line)
   except json.JSONDecodeError: continue
   if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name'] in ['fence','dispatch']:
    dest=Path(f'target/sqlite-scope-latency-{label}-{item["target"]["name"]}')
    shutil.copy2(item['executable'],dest)
    metadata[str(dest)]=dict(sha256=hashlib.sha256(dest.read_bytes()).hexdigest(),tasks_sha256=hashlib.sha256(contents).hexdigest(),command=command)
finally:
 p.write_bytes(candidate)
Path('target/sqlite-scope-latency-build.json').write_text(json.dumps(metadata,indent=2)+'\n')
assert len(metadata)==4
runner=Path('target/compare-fence-wait.py').read_text().replace('target/fence-wait-','target/sqlite-scope-latency-')
Path('target/compare-sqlite-scope-latency.py').write_text(runner)
print('Both release variants built and candidate source restored.')
