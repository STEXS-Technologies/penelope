from pathlib import Path
import os,subprocess,json,time
out=Path(__file__).resolve().parent
cpu=min(os.sched_getaffinity(0))
meta={'cpu':cpu,'pairs':24,'warmup':16,'iterations':250,'start_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'note':'Each variant runs ordinary allocator latency, then a separate Rust allocation traffic probe. AB/BA alternating pairs. No other agent-owned builds/tests during measurement.'}
(out/'campaign.json').write_text(json.dumps(meta,indent=2)+'\n')
with (out/'runs.jsonl').open('w') as log:
 for pair in range(24):
  order=['baseline','candidate'] if pair%2==0 else ['candidate','baseline']
  for variant in order:
   for mode in ['latency','allocations']:
    result=subprocess.run(['taskset','-c',str(cpu),str(out/(variant+'-'+mode))],capture_output=True,text=True,check=True,timeout=60)
    for line in result.stdout.splitlines():
     row=json.loads(line);row.update(pair=pair,variant=variant,mode=mode);log.write(json.dumps(row)+'\n')
    log.flush()
