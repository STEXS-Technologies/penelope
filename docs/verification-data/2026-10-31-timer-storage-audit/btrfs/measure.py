from pathlib import Path
import hashlib,json,os,subprocess,time
out=Path(__file__).resolve().parent;root=Path.cwd();available=sorted(os.sched_getaffinity(0));assert len(available)>=2
binary=root/'target/release/deps/durable_timer_workload-d85c162325b2d2e0'
metadata=json.loads((root/'target/durable-timers-baseline/metadata.json').read_text());assert hashlib.sha256(binary.read_bytes()).hexdigest()==metadata['binary_sha256']
database_root=out/'databases';database_root.mkdir();env=dict(os.environ,TMPDIR=str(database_root));cpus={'one':available[:1],'two':available[:2]};assert not (out/'raw.jsonl').exists()
start=time.time()
with (out/'raw.jsonl').open('w') as log:
 for campaign in range(2):
  for block in range(6):
   order=['one','two','two','one'] if block%2==0 else ['two','one','one','two']
   for slot,variant in enumerate(order):
    rows=[json.loads(line) for line in subprocess.check_output(['taskset','-c',','.join(map(str,cpus[variant])),str(binary),'--batch','128','--width','128','--work','0','--rounds','2','--reopen','false'],text=True,env=env).splitlines()]
    assert len(rows)==2 and all(r['verified'] and r['consumed']==128 for r in rows)
    for row in rows:log.write(json.dumps(row|dict(campaign=campaign,block=block,slot=slot,variant=variant))+'\n')
   log.flush()
(out/'metadata.json').write_text(json.dumps(dict(binary_sha256=metadata['binary_sha256'],cpus=cpus,database_root=str(database_root),filesystem=subprocess.check_output(['findmnt','-T',str(database_root),'-n','-o','FSTYPE,SOURCE'],text=True).strip(),source_metadata=metadata,start=start,end=time.time(),scope='same binary, serial business thread and SQLite worker; different allowed CPU budgets'),indent=2)+'\n')
