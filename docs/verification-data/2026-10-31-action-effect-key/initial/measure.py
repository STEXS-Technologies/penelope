from pathlib import Path
import json,os,platform,subprocess,time
out=Path(__file__).resolve().parent;assert not (out/'domain-raw.jsonl').exists()
cpu=min(os.sched_getaffinity(0));start=time.time()
for family,binary,count in [('domain','action_effect_key',40),('public','lease_claim_profiles',96)]:
 with (out/(family+'-raw.jsonl')).open('w') as raw:
  for campaign in range(2):
   for block in range(6):
    order=['baseline','candidate','candidate','baseline'] if block%2==0 else ['candidate','baseline','baseline','candidate']
    for slot,variant in enumerate(order):
     result=subprocess.check_output(['taskset','-c',str(cpu),str(out/(variant+'-'+binary))],text=True)
     rows=[json.loads(line) for line in result.splitlines()];assert len(rows)==count
     for row in rows:raw.write(json.dumps(row|{'campaign':campaign,'block':block,'slot':slot,'variant':variant})+'\n')
for family,binary,count in [('domain','action_effect_allocations',40),('public','lease_claim_profile_allocations',96)]:
 with (out/(family+'-allocations.jsonl')).open('w') as raw:
  for repeat in range(2):
   for variant in ['baseline','candidate']:
    result=subprocess.check_output(['taskset','-c',str(cpu),str(out/(variant+'-'+binary))],text=True)
    rows=[json.loads(line) for line in result.splitlines()];assert len(rows)==count
    for row in rows:raw.write(json.dumps(row|{'repeat':repeat,'variant':variant})+'\n')
model=next(line.split(':',1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name'))
(out/'measurement.json').write_text(json.dumps({'start':start,'end':time.time(),'cpu':cpu,'cpu_model':model,'platform':platform.platform(),'ordinary_processes':96,'allocation_processes':8},indent=2)+'\n')
