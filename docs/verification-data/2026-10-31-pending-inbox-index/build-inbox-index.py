from pathlib import Path
import subprocess,os,shutil,json,hashlib
source=Path('crates/penelope-sqlite/src/schema.rs');candidate=source.read_bytes();baseline='bd0cdf5bfcced391d59ce11d58831dbc6a764f88'
meta={'baseline':baseline,'compiler':subprocess.check_output(['rustc','-Vv'],text=True),'variants':{},'fixture_sha256':hashlib.sha256(Path('crates/penelope-sqlite/benches/pending_history.rs').read_bytes()).hexdigest()}
env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':''}
try:
 for variant in ('baseline','indexed'):
  content=subprocess.check_output(['git','show',baseline+':'+str(source)]) if variant=='baseline' else candidate
  source.write_bytes(content)
  result=subprocess.run(['cargo','build','-p','penelope-sqlite','--release','--bench','pending_history','--message-format=json'],env=env,text=True,capture_output=True)
  Path(f'target/inbox-index-build-{variant}.log').write_text(result.stdout+result.stderr);result.check_returncode()
  artifacts=[item for item in map(json.loads,result.stdout.splitlines()) if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name']=='pending_history'];assert len(artifacts)==1
  binary=Path(artifacts[0]['executable']);destination=Path('target/inbox-index-'+variant)
  shutil.copyfile(binary,destination);destination.chmod(0o755)
  meta['variants'][variant]={'source_sha256':hashlib.sha256(content).hexdigest(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'path':str(destination)}
  Path(f'target/inbox-index-source-{variant}.rs').write_bytes(content)
  print('built',variant,flush=True)
finally:source.write_bytes(candidate)
Path('target/inbox-index-build.json').write_text(json.dumps(meta,indent=2)+'\n')
