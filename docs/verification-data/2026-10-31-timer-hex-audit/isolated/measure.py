from pathlib import Path
import json,os,platform,subprocess,time
out=Path(__file__).resolve().parent;cpu=min(os.sched_getaffinity(0));records={}
families=[('helper','timer_input','timer_input_allocations',16),('public','lease_batch_services','lease_batch_service_allocations',72)]
for family,binary,_,count in families:
 dest=out/family;dest.mkdir(exist_ok=True);assert not (dest/'raw.jsonl').exists();records[family]={'timing_start':time.time()}
 with (dest/'raw.jsonl').open('w') as raw:
  for campaign in range(2):
   for block in range(6):
    order=['baseline','candidate','candidate','baseline'] if block%2==0 else ['candidate','baseline','baseline','candidate']
    for slot,variant in enumerate(order):
     for args in ([['--outbox-only'],['--timers-only']] if family=='public' else [[]]):
      rows=[json.loads(s) for s in subprocess.check_output(['taskset','-c',str(cpu),str(out/(variant+'-'+binary)),*args],text=True).splitlines()];assert len(rows)==(count//2 if args else count)
      if args: assert all(row['timers']==(args==['--timers-only']) for row in rows)
      for row in rows:raw.write(json.dumps(row|{'campaign':campaign,'block':block,'slot':slot,'variant':variant})+'\n')
 records[family]['timing_end']=time.time()
for family,_,binary,count in families:
 dest=out/family;records[family]['allocation_start']=time.time()
 with (dest/'allocations.jsonl').open('w') as raw:
  for repeat in range(2):
   for variant in ['baseline','candidate']:
    for args in ([['--outbox-only'],['--timers-only']] if family=='public' else [[]]):
     rows=[json.loads(s) for s in subprocess.check_output(['taskset','-c',str(cpu),str(out/(variant+'-'+binary)),*args],text=True).splitlines()];assert len(rows)==(count//2 if args else count)
     if args: assert all(row['timers']==(args==['--timers-only']) for row in rows)
     for row in rows:raw.write(json.dumps(row|{'repeat':repeat,'variant':variant})+'\n')
 records[family]['allocation_end']=time.time()
 model=next(line.split(':',1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name'))
 (dest/'measurement.json').write_text(json.dumps(records[family]|{'cpu':cpu,'cpu_model':model,'platform':platform.platform(),'ordinary_processes':96 if family=='public' else 48,'allocation_processes':8 if family=='public' else 4},indent=2)+'\n')
