"""Separate allocation runs; instrumented elapsed times are deliberately ignored."""
from pathlib import Path
import json,subprocess
out=Path(__file__).resolve().parent
with (out/'allocations.jsonl').open('w') as log:
 for pair in range(4):
  for variant in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']):
   result=subprocess.run(['taskset','-c','0',str(out/(variant+'-allocations-lease_batch'))],text=True,capture_output=True,check=True)
   observations=[json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
   assert len(observations)==64
   log.write(json.dumps({'pair':pair,'variant':variant,'observations':observations})+'\n');log.flush()
  print('allocation pair',pair+1,flush=True)
