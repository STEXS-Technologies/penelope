from pathlib import Path
import os,sys,subprocess,json,time,platform
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
if (out/'raw.jsonl').exists() or (out/'raw.jsonl.gz').exists():raise FileExistsError('timing output already exists')
cpu=min(os.sched_getaffinity(0));env=dict(os.environ,PENELOPE_OUTCOME_LOG_FULL='1');started=time.time()
with (out/'raw.jsonl').open('w') as log:
 for campaign in range(2):
  for block in range(6):
   order=['baseline','candidate','candidate','baseline'] if block%2==0 else ['candidate','baseline','baseline','candidate']
   for slot,label in enumerate(order):
    r=subprocess.run(['taskset','-c',str(cpu),str(out/f'{label}-outcome_log')],env=env,capture_output=True,text=True,check=True)
    rows=[json.loads(line) for line in r.stdout.splitlines()];assert len(rows)==16
    for row in rows:
     row.update(campaign=campaign,block=block,slot=slot,variant=label);log.write(json.dumps(row)+'\n')
    log.flush()
with (out/'allocations.jsonl').open('w') as log:
 for label in ['baseline','candidate']:
  r=subprocess.run(['taskset','-c',str(cpu),str(out/f'{label}-outcome_log_allocations')],env=env,capture_output=True,text=True,check=True)
  rows=[json.loads(line) for line in r.stdout.splitlines()];assert len(rows)==128
  for row in rows:
   row['variant']=label;log.write(json.dumps(row)+'\n')
(out/'measurement.json').write_text(json.dumps({'started_unix':started,'ended_unix':time.time(),'cpu':cpu,'cpu_model':next(s.split(':',1)[1].strip() for s in Path('/proc/cpuinfo').read_text().splitlines() if s.startswith('model name')),'platform':platform.platform(),'ordinary_processes':48,'allocation_processes':2,'timed_logs':396288,'timed_outcomes':75099648,'timed_append_calls':25555968},indent=2)+'\n')
