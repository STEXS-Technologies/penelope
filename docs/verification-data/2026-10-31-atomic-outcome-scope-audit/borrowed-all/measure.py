from pathlib import Path
import itertools,json,os,platform,subprocess,sys,time
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
if (out/'raw.jsonl').exists() or (out/'raw.jsonl.gz').exists():raise FileExistsError('use a fresh output directory')
cpu=min(os.sched_getaffinity(0));started=time.time()
profiles=list(itertools.product([False,True],repeat=3))
def run(label,kind,long,many,varying):
 env=dict(os.environ)
 for key,enabled in [('PENELOPE_BENCH_LONG_IDS',long),('PENELOPE_BENCH_MANY_OUTCOMES',many),('PENELOPE_BENCH_VARY_PAYLOAD',varying)]:
  env.pop(key,None)
  if enabled:env[key]='1'
 result=subprocess.run(['taskset','-c',str(cpu),str(out/(label+'-'+kind))],env=env,capture_output=True,text=True,check=True)
 rows=[]
 for line in result.stdout.splitlines():
  fields=dict(token.split('=',1) for token in line.split())
  row={k:(json.loads(v) if k=='checksum' else int(v)) for k,v in fields.items()}
  row.update(long=long,outcomes=128 if many else 1,varying=varying,variant=label)
  rows.append(row)
 assert len(rows)==4
 return rows
with (out/'raw.jsonl').open('w') as log:
 for campaign in range(2):
  for block in range(6):
   order=['baseline','candidate','candidate','baseline'] if block%2==0 else ['candidate','baseline','baseline','candidate']
   for long,many,varying in profiles:
    for slot,label in enumerate(order):
     for row in run(label,'atomic_validation',long,many,varying):
      row.update(campaign=campaign,block=block,slot=slot);log.write(json.dumps(row)+'\n')
     log.flush()
with (out/'allocations.jsonl').open('w') as log:
 for repeat in range(2):
  for long,many,varying in profiles:
   for label in ['baseline','candidate']:
    for row in run(label,'atomic_validation_allocations',long,many,varying):
     row['repeat']=repeat;log.write(json.dumps(row)+'\n')
(out/'measurement.json').write_text(json.dumps({'started_unix':started,'ended_unix':time.time(),'cpu':cpu,'cpu_model':next(s.split(':',1)[1].strip() for s in Path('/proc/cpuinfo').read_text().splitlines() if s.startswith('model name')),'platform':platform.platform(),'ordinary_processes':384,'allocation_processes':32},indent=2)+'\n')
print('Completed two campaigns: 384 ordinary processes / 1,536 timing rows; allocation probes separate.')
