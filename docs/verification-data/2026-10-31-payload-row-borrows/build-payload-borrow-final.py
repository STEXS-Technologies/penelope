from pathlib import Path
import subprocess,os,shutil,json,hashlib
previous=json.loads(Path('target/payload-borrow-build.json').read_text())
source=Path('crates/penelope-sqlite/src/payloads.rs');content=source.read_bytes()
fixtures={name:hashlib.sha256(Path(name).read_bytes()).hexdigest() for name in previous['fixture_sha256']};assert fixtures==previous['fixture_sha256']
assert subprocess.check_output(['rustc','-Vv'],text=True)==previous['compiler']
meta={**previous,'variants':{'baseline':previous['variants']['baseline'],'nested':previous['variants']['borrowed']}}
env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':''}
result=subprocess.run(['cargo','build','-p','penelope-sqlite','--release','--bench','process_payloads','--bench','process_payload_allocations','--message-format=json'],env=env,text=True,capture_output=True)
Path('target/payload-borrow-final-build-borrowed.log').write_text(result.stdout+result.stderr);result.check_returncode();bins={}
for item in map(json.loads,result.stdout.splitlines()):
 if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name'] in ('process_payloads','process_payload_allocations'):
  name=item['target']['name'];binary=Path(item['executable']);destination=Path('target/payload-borrow-final-borrowed-'+name)
  shutil.copyfile(binary,destination);destination.chmod(0o755)
  bins[name]={'path':str(destination),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest()}
assert len(bins)==2
meta['variants']['borrowed']={'source_sha256':hashlib.sha256(content).hexdigest(),'binaries':bins}
for variant,previous_name in [('baseline','baseline'),('nested','borrowed')]:
 Path(f'target/payload-borrow-final-source-{variant}.rs').write_bytes(Path(f'target/payload-borrow-source-{previous_name}.rs').read_bytes())
Path('target/payload-borrow-final-source-borrowed.rs').write_bytes(content)
Path('target/payload-borrow-final-build.json').write_text(json.dumps(meta,indent=2)+'\n')
print('Retained original binaries and built simplified result variant from identical fixtures')
