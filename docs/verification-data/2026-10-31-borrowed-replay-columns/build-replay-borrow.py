from pathlib import Path
import subprocess,os,shutil,json,hashlib
paths=[Path('crates/penelope-sqlite/src/process.rs'),Path('crates/penelope-sqlite/src/tasks.rs')]
candidate={str(path):path.read_bytes() for path in paths};base='ec4399f0cc27698a63b7c9ac23e408b0b6557785'
meta={'baseline':base,'compiler':subprocess.check_output(['rustc','-Vv'],text=True),'variants':{}}
env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':''}
try:
 for variant in ('baseline','borrowed'):
  contents={str(path):subprocess.check_output(['git','show',base+':'+str(path)]) if variant=='baseline' else candidate[str(path)] for path in paths}
  for path in paths:path.write_bytes(contents[str(path)])
  result=subprocess.run(['cargo','build','-p','penelope-sqlite','--release','--bench','process_replay','--bench','process_replay_allocations','--message-format=json'],env=env,text=True,capture_output=True)
  Path(f'target/replay-borrow-build-{variant}.log').write_text(result.stdout+result.stderr);result.check_returncode()
  bins={}
  for item in map(json.loads,result.stdout.splitlines()):
   if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name'] in ('process_replay','process_replay_allocations'):
    name=item['target']['name'];binary=Path(item['executable']);destination=Path('target/replay-borrow-'+variant+'-'+name)
    shutil.copyfile(binary,destination);destination.chmod(0o755)
    bins[name]={'path':str(destination),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest()}
  assert len(bins)==2
  meta['variants'][variant]={'source_sha256':{name:hashlib.sha256(content).hexdigest() for name,content in contents.items()},'binaries':bins}
  for name,content in contents.items():Path(f'target/replay-borrow-source-{variant}-{Path(name).name}').write_bytes(content)
  print('built',variant,flush=True)
finally:
 for path in paths:path.write_bytes(candidate[str(path)])
meta['fixture_sha256']={name:hashlib.sha256(Path(name).read_bytes()).hexdigest() for name in ('crates/penelope-sqlite/benches/process_replay.rs','crates/penelope-sqlite/benches/process_replay_allocations.rs','crates/penelope-sqlite/benches/support/process_replay_fixture.rs')}
Path('target/replay-borrow-build.json').write_text(json.dumps(meta,indent=2)+'\n')
