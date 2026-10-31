from pathlib import Path
import subprocess,os,shutil,json,hashlib
source=Path('crates/penelope-sqlite/src/payloads.rs');candidate=source.read_bytes();baseline='ea5a230e847a6eca9c088e8811c458d92fc37d87'
meta={'baseline':baseline,'compiler':subprocess.check_output(['rustc','-Vv'],text=True),'variants':{}}
env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','TMPDIR':str(Path('target/qualification-temp').resolve())}
try:
 for variant in ('baseline','borrowed'):
  content=subprocess.check_output(['git','show',baseline+':'+str(source)]) if variant=='baseline' else candidate
  source.write_bytes(content);bins={}
  result=subprocess.run(['cargo','build','-p','penelope-sqlite','--release','--bench','process_payloads','--bench','process_payload_allocations','--message-format=json'],env=env,text=True,capture_output=True)
  Path(f'target/payload-borrow-build-{variant}.log').write_text(result.stdout+result.stderr);result.check_returncode()
  for item in map(json.loads,result.stdout.splitlines()):
   if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name'] in ('process_payloads','process_payload_allocations'):
    name=item['target']['name'];binary=Path(item['executable']);destination=Path('target/payload-borrow-'+variant+'-'+name)
    shutil.copyfile(binary,destination);destination.chmod(0o755)
    bins[name]={'path':str(destination),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest()}
  assert len(bins)==2
  meta['variants'][variant]={'source_sha256':hashlib.sha256(content).hexdigest(),'binaries':bins}
  Path(f'target/payload-borrow-source-{variant}.rs').write_bytes(content)
  if variant=='baseline':
   parity=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','payload_columns::','--','--nocapture','--test-threads=1'],env=env,text=True,capture_output=True)
   Path('target/payload-borrow-baseline-regression.log').write_text(parity.stdout+parity.stderr);parity.check_returncode()
  print('built',variant,flush=True)
finally:source.write_bytes(candidate)
meta['fixture_sha256']={name:hashlib.sha256(Path(name).read_bytes()).hexdigest() for name in ('crates/penelope-sqlite/benches/process_payloads.rs','crates/penelope-sqlite/benches/process_payload_allocations.rs','crates/penelope-sqlite/benches/support/payload_fixture.rs')}
Path('target/payload-borrow-build.json').write_text(json.dumps(meta,indent=2)+'\n')
