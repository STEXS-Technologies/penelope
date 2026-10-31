from pathlib import Path
import os,subprocess,json,time
out=Path(__file__).resolve().parent;cpu=min(os.sched_getaffinity(0))
(out/'campaign.json').write_text(json.dumps({'cpu':cpu,'pairs':24,'order':'alternating AB/BA; blob latency then allocation; reconstruction latency then allocation','start_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'note':'No agent-owned builds/tests/graph jobs during timing. Separate ordinary allocator latency and Rust allocation traffic executables.'},indent=2)+'\n')
with (out/'runs.jsonl').open('w') as log:
 for pair in range(24):
  for variant in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']):
   for workload,mode in [('blob','latency'),('blob','allocations'),('reconstruction','latency'),('reconstruction','allocations')]:
    process=subprocess.run(['taskset','-c',str(cpu),str(out/(variant+'-'+workload+'-'+mode))],capture_output=True,text=True,timeout=60)
    if process.returncode:
     (out/'failed-process.stderr').write_text(process.stderr);process.check_returncode()
    for line in process.stdout.splitlines():
     row=json.loads(line);profile=f"blob/{row['size']}/{int(row['corrupt'])}" if workload=='blob' else f"reconstruction/{row['history']}"
     row.update(pair=pair,variant=variant,workload=workload,mode=mode,profile=profile);log.write(json.dumps(row)+'\n')
    log.flush()
