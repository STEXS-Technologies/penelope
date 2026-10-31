from pathlib import Path
import hashlib,json,os,subprocess
root=Path.cwd();out=Path(__file__).resolve().parent
services='crates/penelope-executor/src/services.rs';identity='crates/penelope-executor/src/timer_input.rs';leases='crates/penelope-sqlite/src/leases.rs';process='crates/penelope-sqlite/src/process.rs';original={p:(root/p).read_bytes() for p in [services,identity,leases,process]}
s=original[services];h=original[identity];l=original[leases];p=original[process]
ack=b'''        timers
            .acknowledge(&lease, now)
            .await
            .map_err(|error| fail(TimerDrainStage::Acknowledge, report, error))?;
        report.acknowledged = report.acknowledged.saturating_add(1);'''
premature=b'''        report.acknowledged = report.acknowledged.saturating_add(1);
        timers
            .acknowledge(&lease, now)
            .await
            .map_err(|error| fail(TimerDrainStage::Acknowledge, report, error))?;'''
receipt=b'''        receipt
            .validate_for(&input)
            .map_err(|_invalid| fail(TimerDrainStage::Inbox, report, PortError::Invariant))?;
'''
mutants=[
 ('report',services,s.replace(ack,premature),'17-9-8-AckAfter','assertion `left == right` failed'),
 ('receipt',services,s.replace(receipt,b''),'17-9-8-ReceiptMismatch','on an `Ok` value'),
 ('identity',identity,h.replace(b'ContentDigest::sha256(&key)',b'if lease.owner.as_str() == "pri_recovery" { key.extend_from_slice(lease.owner.as_str().as_bytes()); }\n        ContentDigest::sha256(&key)'),'17-9-8-AckBefore','assertion `left == right` failed'),
 ('expiry',leases,l.replace(b'state=0 AND due_at<=?2 AND (expires_at IS NULL OR expires_at<?2)',b'state=0 AND due_at<=?2 AND (expires_at IS NULL OR expires_at<=?2)'),'17-9-8-AckAfter','assertion `left == right` failed'),
 ('continue-after-failure',services,s.replace(ack,b'''        let _ignored = timers.acknowledge(&lease, now).await;
        report.acknowledged = report.acknowledged.saturating_add(1);'''),'17-9-8-AckBefore','on an `Ok` value'),
 ('consume-twice',process,p.replace(b'        if consumed {',b'        if consumed && false {'),'17-9-8-AckAfter','assertion `left == right` failed'),
]
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1');results=[]
try:
 for name,path,source,case,marker in mutants:
  for file,data in original.items():(root/file).write_bytes(data)
  assert source!=original[path];(root/path).write_bytes(source)
  with (out/('negative-'+name+'.txt')).open('w') as log:r=subprocess.run(['cargo','test','-p','penelope-executor','--test','timer_batch_recovery','--','--nocapture'],env=env|{'PENELOPE_TIMER_BATCH_CASE':case},stdout=log,stderr=subprocess.STDOUT)
  text=(out/('negative-'+name+'.txt')).read_text();assert r.returncode!=0 and marker in text and 'panicked at' in text,(name,text)
  results.append({'name':name,'case':case,'marker':marker,'rejected':True})
finally:
 for file,data in original.items():(root/file).write_bytes(data)
assert all((root/file).read_bytes()==data for file,data in original.items())
(out/'negative-results.json').write_text(json.dumps({'results':results,'restored_sha256':{p:hashlib.sha256(b).hexdigest() for p,b in original.items()}},indent=2)+'\n')
