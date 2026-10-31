from pathlib import Path
import subprocess,os,shutil,json,hashlib
source=Path('crates/penelope-sqlite/src/process.rs');candidate=source.read_bytes()
variants={'pre_frontier':'837265bfc82658a6d1fe02e4776372fe4673d805','frontier':'465b6b267afe68796e21e133f971d5ad8796cbc9','cached':None}
meta={'compiler':subprocess.check_output(['rustc','-Vv'],text=True),'variants':{},'fixture_sha256':hashlib.sha256(Path('crates/penelope-sqlite/benches/process_replay.rs').read_bytes()).hexdigest()}
env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':''}
try:
 for name,pin in variants.items():
  content=subprocess.check_output(['git','show',pin+':'+str(source)]) if pin else candidate
  source.write_bytes(content)
  result=subprocess.run(['cargo','build','-p','penelope-sqlite','--release','--bench','process_replay','--message-format=json'],env=env,text=True,capture_output=True)
  Path(f'target/replay-cache-build-{name}.log').write_text(result.stdout+result.stderr);result.check_returncode()
  artifacts=[item for item in map(json.loads,result.stdout.splitlines()) if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name']=='process_replay']
  assert len(artifacts)==1
  binary=Path(artifacts[0]['executable']);destination=Path('target/replay-cache-'+name)
  shutil.copyfile(binary,destination);destination.chmod(0o755)
  Path('target/replay-cache-source-'+name+'.rs').write_bytes(content)
  meta['variants'][name]={'pin':pin,'source_sha256':hashlib.sha256(content).hexdigest(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'path':str(destination)}
  print('built',name,flush=True)
finally:source.write_bytes(candidate)
Path('target/replay-cache-build.json').write_text(json.dumps(meta,indent=2)+'\n')
