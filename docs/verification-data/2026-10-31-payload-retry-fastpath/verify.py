"""Verify source-pinned retry evidence without changing or timing production code."""
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
m=json.loads(read(out,'metadata.json'));assert m['parent']=='704688d5ec28eca0d277165e8a365d7d52edab3f'
runtime='crates/penelope-sqlite/src/payloads.rs'
assert read(out,'baseline-payloads.rs')==subprocess.check_output(['git','show',m['parent']+':'+runtime],cwd=root)
assert read(out,'candidate-payloads.rs')==(root/runtime).read_bytes()
for variant in ['baseline','candidate']:assert m['runtime_sources'][variant]=={runtime:sha(read(out,variant+'-payloads.rs'))}
assert len(m['binaries'])==6 and all(len(digest)==64 for digest in m['binaries'].values())
for path,digest in m['common_sha256'].items():assert sha((root/path).read_bytes())==digest,path
for path,digest in json.loads(read(out,'source-hashes.json')).items():assert sha((root/path).read_bytes())==digest,path
for variant,count in [('baseline',72),('candidate',74)]:
 text=read(out,variant+'-tests.txt').decode();assert f'{count} passed; 0 failed' in text and 'test result: FAILED' not in text
 for package in ['penelope-sqlite','penelope-executor']:assert 'Finished' in read(out,variant+'-'+package+'-build.txt').decode()
assert 'payload_retry_lock_cases=12' in read(out,'candidate-tests.txt').decode()
assert 'payload_retry_corruption_cases=12' in read(out,'candidate-tests.txt').decode()
assert 'payload_insert_race=true concurrent_insert_rechecked=true quota_charged_once=true' in read(out,'candidate-race.txt').decode()
summaries={}
for family in ['private','public']:
 directory=out/family
 with tempfile.TemporaryDirectory(prefix='penelope-payload-retry-') as folder:
  temporary=Path(folder)
  for name in ['raw.jsonl.gz','measurement.json']+(['allocations.jsonl.gz'] if family=='private' else []):shutil.copy2(directory/name,temporary/name)
  subprocess.run([sys.executable,str(out/'analyze.py'),family,str(temporary)],check=True,stdout=subprocess.DEVNULL)
  assert (temporary/'summary.json').read_bytes()==read(directory,'summary.json')
 summaries[family]=json.loads(read(directory,'summary.json'))
private=summaries['private'];public=summaries['public'];assert private['rows']==1280 and public['rows']==576 and public['timers']==27840
retry=[r for r in private['latency'] if r['case']=='duplicate'];assert len(retry)==8 and sum(r['ci_high']<0 for r in retry)==7 and all(r['ci_low']<0 for r in retry)
assert all(r['ci_high']<0 for r in private['latency'] if r['case']=='duplicate' and r['size']<1048576)
assert sum(r['ci_high']<0 for r in private['latency'] if r['case']=='corrupt_put')==8
assert sum(r['ci_low']>0 for r in private['latency'] if r['case']=='read')==1
for row in private['allocations']:
 a,b=row['baseline'],row['candidate'];delta=1 if row['case'] in ['duplicate','corrupt_put'] else 0
 assert b['allocations']['median']-a['allocations']['median']==delta
 assert b['requested_bytes']['median']-a['requested_bytes']['median']==delta*80
 assert b['reallocations']['median']==a['reallocations']['median']
total=[r for r in public['latency'] if r['metric']=='managed_ns'];assert len(total)==12 and sum(r['ci_high']<0 for r in total)==2 and all(r['ci_low']<0 for r in total)
assert sum(r['ci_low']>0 for r in public['latency'] if r['metric']=='schedule_ns')==1
assert sum(r['ci_low']>0 for r in public['latency'] if r['metric']=='consume_ns')==1
assert sum(r['ci_low']>0 for r in public['latency'] if r['metric']=='direct_ns')==2
q=out/'qualification';results=json.loads(read(q,'results.json'))
assert set(results)=={'workspace','msrv','clippy','msrv-check','fmt','layers','graph'} and all(value['exit_code']==0 for value in results.values())
for name in ['workspace','msrv']:
 text=read(q,name+'.txt').decode();assert '0 failed' in text and 'test result: FAILED' not in text
 for marker in ['test payloads::tests::insertion_after_read_is_rechecked_before_quota_charge ... ok','test payload_retry_lock::exact_payload_retries_do_not_require_the_writer_lock ... ok','test payload_retry_lock::locked_payload_retries_revalidate_committed_corruption ... ok','test durable_timer_batches_recover_partial_delivery_without_reconsuming_inputs ... ok']:assert marker in text,marker
for name in ['clippy','msrv-check']:assert 'Finished' in read(q,name+'.txt').decode() and 'error:' not in read(q,name+'.txt').decode()
assert read(q,'fmt.txt')==read(q,'layers.txt')==b'' and b'Code graph updated.' in read(q,'graph.txt')
negative=json.loads(read(out/'negative','results.json'));assert {r['name'] for r in negative['results']}=={'baseline-lock','byte-equality','scope','recheck'}
for result in negative['results']:
 text=read(out/'negative',result['name']+'.txt').decode();assert result['rejected'] and result['marker'] in text and 'test result: FAILED' in text
assert negative['restored_sha256']==m['runtime_sources']['candidate'][runtime]
assert '2 passed; 0 failed' in read(out/'negative','restored.txt').decode()
print('payload retry evidence verified: 24 public lock/corruption cases, deterministic insertion race, 1,280 payload timing rows, 576 durable workflows/27,840 timers, 2,560 allocation observations, 4 rejected controls, exact summaries and source restoration')
