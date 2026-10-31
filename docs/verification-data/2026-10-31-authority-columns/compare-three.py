from pathlib import Path
import os,subprocess,json,time,itertools
out=Path(__file__).resolve().parent;cpu=min(os.sched_getaffinity(0));orders=list(itertools.permutations(['baseline','uncached','cached']))
(out/'final-campaign.json').write_text(json.dumps({'cpu':cpu,'blocks':24,'orders':[list(o) for o in orders],'order_repeats':4,'warmup':16,'iterations':250,'start_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'note':'Separate ordinary latency and allocation traffic executables. No other agent-owned builds/tests/graph jobs during timing.'},indent=2)+'\n')
with (out/'final-runs.jsonl').open('w') as log:
 for block in range(24):
  for variant in orders[block%6]:
   for mode in ['latency','allocations']:
    result=subprocess.run(['taskset','-c',str(cpu),str(out/(variant+'-'+mode))],capture_output=True,text=True,timeout=60)
    if result.returncode:(out/'final-failure.stderr').write_text(result.stderr);result.check_returncode()
    for line in result.stdout.splitlines():
     row=json.loads(line);row.update(block=block,variant=variant,mode=mode);log.write(json.dumps(row)+'\n')
    log.flush()
