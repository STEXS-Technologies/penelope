import argparse,subprocess,re,json,os
from pathlib import Path
parser=argparse.ArgumentParser();parser.add_argument('log');parser.add_argument('output');parser.add_argument('--runs',type=int,default=50);args=parser.parse_args()
text=Path(args.log).read_text()
unit=re.search(r'Running unittests src/lib.rs \(([^\n]+/penelope_process_runtime-[^\n]+)\)',text)
integration=re.search(r'Running tests/runtime.rs \(([^\n]+)\)',text)
assert unit and integration
out=Path(args.output)
with out.open('w') as stream:
 for iteration in range(args.runs):
  for binary,pattern,expected in [(unit[1],'bounded_send',4),(unit[1],'startup_parts',2),(integration[1],'expired_startup_write_poisoning_requires_reconstruction_and_fences_worker',1)]:
   command=[binary,pattern,'--test-threads=1']
   p=subprocess.run(command,text=True,capture_output=True,timeout=30,env={**os.environ,'TMPDIR':'/home/ac/projects/penelope/target/qualification-temp'})
   row={'iteration':iteration,'command':command,'returncode':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
   stream.write(json.dumps(row)+'\n');stream.flush()
   assert p.returncode==0,row
   assert f'test result: ok. {expected} passed; 0 failed;' in p.stdout,row
print('Verified',args.runs*7,'test executions in',args.runs*3,'processes')
