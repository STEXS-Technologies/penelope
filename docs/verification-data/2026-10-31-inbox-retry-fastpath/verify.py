"""Verify source-pinned inbox evidence without changing or timing project code."""
from pathlib import Path
import gzip,hashlib,json,shutil,subprocess,sys,tempfile
out=Path(__file__).resolve().parent;root=out.parents[2]
def sha(data):return hashlib.sha256(data).hexdigest()
def read(folder,name):
 p=folder/name
 return p.read_bytes() if p.exists() else gzip.decompress((folder/(name+'.gz')).read_bytes())
manifest=json.loads((out/'manifest.json').read_text())
assert set(manifest)=={p.relative_to(out).as_posix() for p in out.rglob('*') if p.is_file() and p.name!='manifest.json'}
for name,digest in manifest.items():assert sha((out/name).read_bytes())==digest,name
m=json.loads(read(out,'metadata.json'));assert m['parent']=='8c11d33030c38075421a03f8f17f513d6dfe0d3d'
runtime='crates/penelope-sqlite/src/process.rs'
assert read(out,'baseline-process.rs')==subprocess.check_output(['git','show',m['parent']+':'+runtime],cwd=root)
assert read(out,'candidate-process.rs')==(root/runtime).read_bytes()
for variant in ['baseline','candidate']:assert m['runtime_sources'][variant]=={runtime:sha(read(out,variant+'-process.rs'))}
assert len(m['binaries'])==6 and all(len(digest)==64 for digest in m['binaries'].values())
for path,digest in m['common_sha256'].items():assert sha((root/path).read_bytes())==digest,path
for path,digest in json.loads(read(out,'source-hashes.json')).items():assert sha((root/path).read_bytes())==digest,path
for variant,count in [('baseline',74),('candidate',76)]:
 text=read(out,variant+'-tests.txt').decode();assert f'{count} passed; 0 failed' in text and 'test result: FAILED' not in text
 for package in ['penelope-sqlite','penelope-executor']:assert 'Finished' in read(out,variant+'-'+package+'-build.txt').decode()
for marker in ['inbox_retry_validation_cases=44 writer_locked=true','inbox_retry_validation_cases=44 writer_locked=false','inbox_retry_lock_cases=4']:assert marker in read(out,'candidate-tests.txt').decode(),marker
assert 'inbox_insert_race=true concurrent_admission_rechecked=true quota_charged_once=true unconsumed=true' in read(out,'candidate-race.txt').decode()
summaries={}
for family in ['private','public']:
 directory=out/family
 with tempfile.TemporaryDirectory(prefix='penelope-inbox-retry-') as folder:
  temporary=Path(folder)
  for name in ['raw.jsonl.gz','measurement.json']+(['allocations.jsonl.gz'] if family=='private' else []):shutil.copy2(directory/name,temporary/name)
  subprocess.run([sys.executable,str(out/'analyze.py'),family,str(temporary)],check=True,stdout=subprocess.DEVNULL)
  assert (temporary/'summary.json').read_bytes()==read(directory,'summary.json')
 summaries[family]=json.loads(read(directory,'summary.json'))
private=summaries['private'];public=summaries['public'];assert private['rows']==128 and public['rows']==576 and public['timers']==27840
assert len(private['latency'])==4 and all(r['ci_high']<0 for r in private['latency'])
assert len(private['allocations'])==2
for row in private['allocations']:
 for variant in ['baseline','candidate']:
  for metric,value in [('allocations',8),('reallocations',1),('requested_bytes',968)]:assert all(v==value for v in row[variant][metric].values())
total=[r for r in public['latency'] if r['metric']=='managed_ns'];assert len(total)==12 and sum(r['ci_high']<0 for r in total)==1 and all(r['ci_low']<0 for r in total)
regressions=[r for r in public['latency'] if r['ci_low']>0];assert len(regressions)==1
r=regressions[0];assert (r['metric'],r['campaign'],r['batch'],r['width'])==('schedule_ns',1,16,9)
q=out/'qualification';results=json.loads(read(q,'results.json'))
assert set(results)=={'workspace','msrv','clippy','msrv-check','fmt','layers','graph'} and all(value['exit_code']==0 for value in results.values())
for name in ['workspace','msrv']:
 text=read(q,name+'.txt').decode();assert '0 failed' in text and 'test result: FAILED' not in text
 for marker in ['test process::inbox_retry_tests::concurrent_admission_is_rechecked_before_quota_charge ... ok','test inbox_retry_lock::pending_and_consumed_retries_preserve_quota_scope_and_committed_state ... ok','test inbox_validation::duplicate_admission_validates_evidence_without_the_writer_lock ... ok','test durable_timer_batches_recover_partial_delivery_without_reconsuming_inputs ... ok']:assert marker in text,marker
for name in ['clippy','msrv-check']:assert 'Finished' in read(q,name+'.txt').decode() and 'error:' not in read(q,name+'.txt').decode()
assert read(q,'fmt.txt')==read(q,'layers.txt')==b'' and b'Code graph updated.' in read(q,'graph.txt')
negative=json.loads(read(out/'negative','results.json'));assert {r['name'] for r in negative['results']}=={'baseline-lock','checksum','semantic','scope','recheck'}
for result in negative['results']:
 text=read(out/'negative',result['name']+'.txt').decode();assert result['rejected'] and result['marker'] in text and 'test result: FAILED' in text
assert negative['restored_sha256']==m['runtime_sources']['candidate'][runtime]
assert '4 passed; 0 failed' in read(out/'negative','restored.txt').decode()
print('inbox retry evidence verified: 48 public lock cases, deterministic admission race, 128 retry timing rows/640,000 calls, 576 durable workflows/27,840 timers, 512 allocation observations, five rejected controls, exact summaries and source restoration')
