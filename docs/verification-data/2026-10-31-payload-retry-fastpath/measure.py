from pathlib import Path
import hashlib,json,os,platform,subprocess,time
out=Path(__file__).resolve().parent;root=Path.cwd();database_root=out/'databases';database_root.mkdir(exist_ok=True);env=dict(os.environ,TMPDIR=str(database_root));cpu=min(os.sched_getaffinity(0))
metadata=json.loads((out/'metadata.json').read_text())
for name,digest in metadata['binaries'].items():assert hashlib.sha256((out/name).read_bytes()).hexdigest()==digest
records={}
for family,blocks in [('private',8),('public',6)]:
 dest=out/family;dest.mkdir(exist_ok=True);assert not (dest/'raw.jsonl').exists();records[family]=dict(start=time.time())
 with (dest/'raw.jsonl').open('w') as log:
  for campaign in range(2):
   for block in range(blocks):
    order=['baseline','candidate','candidate','baseline'] if block%2==0 else ['candidate','baseline','baseline','candidate']
    for slot,variant in enumerate(order):
     if family=='private':
      args=[['process_payloads']]
     else:
      args=[['durable_timer_workload','--batch',str(batch),'--width',str(width),'--work','65536','--rounds','2','--reopen','false'] for batch in [1,16,128] for width in [9,128]]
     for binary,*options in args:
      rows=[json.loads(line) for line in subprocess.check_output(['taskset','-c',str(cpu),str(out/(variant+'-'+binary)),*options],env=env,text=True).splitlines()]
      assert len(rows)==(20 if family=='private' else 2)
      for row in rows:log.write(json.dumps(row|dict(campaign=campaign,block=block,slot=slot,variant=variant))+'\n')
    log.flush();print(f'{family} campaign={campaign} block={block} complete',flush=True)
 records[family]['end']=time.time()
# Instrumented probes only start after both ordinary timing families finish.
with (out/'private/allocations.jsonl').open('w') as log:
 for repeat in range(4):
  for variant in ['baseline','candidate']:
   rows=[json.loads(line) for line in subprocess.check_output(['taskset','-c',str(cpu),str(out/(variant+'-process_payload_allocations'))],env=env,text=True).splitlines()];assert len(rows)==320
   for row in rows:log.write(json.dumps(row|dict(repeat=repeat,variant=variant))+'\n')
for family in ['private','public']:
 (out/family/'measurement.json').write_text(json.dumps(records[family]|dict(cpu=cpu,platform=platform.platform(),database_root=str(database_root),filesystem=subprocess.check_output(['findmnt','-T',str(database_root),'-n','-o','FSTYPE,SOURCE'],text=True).strip(),ordinary_processes=64 if family=='private' else 288,allocation_processes=8 if family=='private' else 0),indent=2)+'\n')
