from pathlib import Path
import subprocess,os,shutil,json,hashlib
source=Path('crates/penelope-sqlite/src/leases.rs');candidate=source.read_bytes()
metadata={'baseline_source':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'compiler':subprocess.check_output(['rustc','-Vv'],text=True),'variants':{}}
env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':''}
try:
 for variant in ['baseline','candidate']:
  source.write_bytes(subprocess.check_output(['git','show','HEAD:'+str(source)]) if variant=='baseline' else candidate)
  with open('target/token-cache-'+variant+'-build-final.log','w') as log:
   subprocess.run(['cargo','build','-p','penelope-sqlite','--bench','lease_batch','--release'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  binary=next(p for p in Path('target/release/deps').glob('lease_batch-*') if p.is_file() and p.suffix!='.d' and p.stat().st_mode&0o111)
  shutil.copyfile(binary,'target/token-cache-'+variant);Path('target/token-cache-'+variant).chmod(0o755)
  metadata['variants'][variant]={'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()}
finally:
 source.write_bytes(candidate)
metadata['benchmark_sha256']=hashlib.sha256(Path('crates/penelope-sqlite/benches/lease_batch.rs').read_bytes()).hexdigest()
Path('target/token-cache-build.json').write_text(json.dumps(metadata,indent=2)+'\n')
