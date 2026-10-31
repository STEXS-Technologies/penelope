from pathlib import Path
import subprocess,os,json,hashlib,shutil
p=Path('crates/penelope-process-runtime/src/registry.rs');candidate=p.read_bytes()
baseline=subprocess.check_output(['git','show','a0cd454:crates/penelope-process-runtime/src/registry.rs'])
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR='/home/ac/projects/penelope/target/qualification-temp')
metadata={}
try:
 for label,contents in [('baseline',baseline),('candidate',candidate)]:
  p.write_bytes(contents)
  with open(f'target/registry-latency-build-{label}.log','w') as log:
   command=['cargo','bench','-p','penelope-process-runtime','--bench','fence','--bench','dispatch','--no-run','--message-format=json']
   run=subprocess.run(command,env=env,stdout=log,stderr=subprocess.STDOUT)
  run.check_returncode()
  for line in Path(f'target/registry-latency-build-{label}.log').read_text().splitlines():
   try: item=json.loads(line)
   except json.JSONDecodeError: continue
   if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name'] in ['fence','dispatch']:
    dest=Path(f'target/registry-latency-{label}-{item["target"]["name"]}')
    shutil.copy2(item['executable'],dest)
    metadata[str(dest)]=dict(sha256=hashlib.sha256(dest.read_bytes()).hexdigest(),registry_sha256=hashlib.sha256(contents).hexdigest(),command=command)
finally:
 p.write_bytes(candidate)
Path('target/registry-latency-build.json').write_text(json.dumps(metadata,indent=2)+'\n')
assert len(metadata)==4
runner=Path('target/compare-fence-wait.py').read_text().replace('target/fence-wait-','target/registry-latency-')
Path('target/compare-registry-latency.py').write_text(runner)
print('Both release variants built and candidate source restored.')
