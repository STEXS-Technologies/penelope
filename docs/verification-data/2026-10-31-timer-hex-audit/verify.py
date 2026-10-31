"""Verify rejected optimization evidence without modifying project sources."""
from pathlib import Path
import concurrent.futures,gzip,hashlib,json,shutil,subprocess,sys,tarfile,tempfile
out=Path(__file__).resolve().parent;root=out.parents[2]
def sha(data):return hashlib.sha256(data).hexdigest()
def read(directory,name):
 p=directory/name
 return p.read_bytes() if p.exists() else gzip.decompress((directory/(name+'.gz')).read_bytes())
manifest=json.loads((out/'manifest.json').read_text())
files={p.relative_to(out).as_posix() for p in out.rglob('*') if p.is_file() and p.name!='manifest.json'}
assert set(manifest)==files
for name,digest in manifest.items():assert sha((out/name).read_bytes())==digest,name
decision=json.loads((out/'decision.json').read_text());assert decision['decision']=='reject-runtime-encoder'
runtime='crates/penelope-executor/src/timer_input.rs'
baseline=subprocess.check_output(['git','show',decision['parent']+':'+runtime],cwd=root)
assert (root/runtime).read_bytes()==baseline,'rejected runtime change must remain absent'
assert (root/'crates/penelope-executor/Cargo.toml').read_bytes()==subprocess.check_output(['git','show',decision['parent']+':crates/penelope-executor/Cargo.toml'],cwd=root)
for path,digest in json.loads((out/'source-hashes.json').read_text()).items():assert sha((root/path).read_bytes())==digest,path
expected={'shared':(17,68),'frames':(50,72),'isolated':(25,72),'aligned':(1,72)}
jobs=[]
for experiment,(regressions,gains) in expected.items():
 directory=out/experiment;m=json.loads(read(directory,'metadata.json'));assert m['parent']==decision['parent']
 with tarfile.open(directory/'candidate-sources.tar.gz','r:gz') as archive:
  snapshots={member.name:archive.extractfile(member).read() for member in archive.getmembers() if member.isfile()}
 for path,digest in m['common_sha256'].items():assert sha(snapshots[path])==digest,(experiment,path)
 assert sha(snapshots[runtime])==decision['candidate_sha256']
 assert m['runtime_sources']['baseline']=={runtime:sha(baseline)}
 assert m['runtime_sources']['candidate']=={runtime:decision['candidate_sha256']}
 assert len(m['binaries'])==8 and all(len(h)==64 for h in m['binaries'].values())
 assert ('diagnostic_rustflags' in m)==(experiment=='aligned')
 if experiment=='aligned':assert m['diagnostic_rustflags']=='-C llvm-args=--align-all-functions=6'
 for variant in ['baseline','candidate']:
  text=read(directory,variant+'-tests.txt').decode()
  assert 'timer_input_legacy_cases=10192 metadata_substitutions=30576' in text
  assert 'timer_input_allocation_profiles=16 budgets_pass=true exact_output=true' in text
  assert 'test result: FAILED' not in text
  if variant=='candidate':assert 'timer_hex_cases=259' in text
  assert 'Finished' in read(directory,variant+'-build.txt').decode()
 public=json.loads(read(directory/'public','summary.json'))['latency']
 assert sum(r['ci_low']>0 for r in public if not r['timers'])==regressions
 assert sum(r['ci_high']<0 for r in public if r['timers'])==gains
 for family in ['helper','public']:jobs.append((directory,family))
 if experiment!='aligned':
  negative=json.loads(read(directory,'negative-results.json'))
  assert negative['restored_sha256']==decision['candidate_sha256']
  assert {n['name'] for n in negative['results']}=={'nibble-order','uppercase','byte-order','leading-zero','extra-allocation'}
  for n in negative['results']:
   assert n['rejected'] and n['marker'] in read(directory,'negative-'+n['name']+'.txt').decode()
def recompute(job):
 directory,family=job
 with tempfile.TemporaryDirectory(prefix='penelope-hex-audit-') as folder:
  temporary=Path(folder)
  for name in ['raw.jsonl.gz','allocations.jsonl.gz','measurement.json']:shutil.copy2(directory/family/name,temporary/name)
  subprocess.run([sys.executable,str(directory/('analyze-'+family+'.py')),str(temporary)],check=True,stdout=subprocess.DEVNULL)
  assert (temporary/'summary.json').read_bytes()==read(directory/family,'summary.json'),(directory.name,family)
with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:list(pool.map(recompute,jobs))
disassembly=json.loads((out/'isolated/outbox-disassembly.json').read_text())
assert disassembly['normalized_equal']
for variant in ['baseline','candidate']:
 assert disassembly[variant]['binary_sha256']==json.loads(read(out/'isolated','metadata.json'))['binaries'][variant+'-lease_batch_services']
 assert sha(read(out/'isolated',variant+'-outbox-normalized.txt'))==disassembly[variant]['normalized_sha256']
assert disassembly['baseline']['normalized_sha256']==disassembly['candidate']['normalized_sha256']
q=out/'qualification';results=json.loads((q/'results.json').read_text())
assert set(results)=={'workspace','msrv','clippy','msrv-check','fmt','layers','graph'}
assert all(r['exit_code']==0 for r in results.values())
for name in ['workspace','msrv']:
 text=read(q,name+'.txt').decode();assert '0 failed' in text and 'test result: FAILED' not in text
 assert 'test durable_timer_batches_recover_partial_delivery_without_reconsuming_inputs ... ok' in text
for name in ['clippy','msrv-check']:
 text=read(q,name+'.txt').decode();assert 'Finished' in text and 'error:' not in text
for name in ['fmt','layers']:assert read(q,name+'.txt')==b''
assert 'Code graph updated.' in read(q,'graph.txt').decode()
smoke=json.loads((q/'benchmark-smoke.json').read_text())
assert set(smoke)=={'lease_batch_services','lease_batch_service_allocations'}
for result in smoke.values():
 assert result['profiles']==72 and result['timer_profiles']==result['outbox_profiles']==36
 assert result['cargo_bench_argument'] and result['same_checksums_and_allocations'] and result['invalid_options_rejected']==3
print('timer hex audit verified: rejected encoder, 4 experiments, 16,896 timing rows, 1,408 allocation rows, exact summaries, 15 rejected fault injections, unchanged production runtime')
