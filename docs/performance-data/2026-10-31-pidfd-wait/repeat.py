import argparse,json,os,re,subprocess
from pathlib import Path
parser=argparse.ArgumentParser();parser.add_argument('log');parser.add_argument('output');parser.add_argument('--runs',type=int,default=50);args=parser.parse_args()
log=Path(args.log).read_text()
unit=re.search(r'Running unittests src/lib.rs \(([^\n]+/penelope_process_runtime-[^\n]+)\)',log)
integration=re.search(r'Running tests/runtime.rs \(([^\n]+)\)',log)
assert unit and integration
cases=[
 (unit[1],'runtime::fence_tests::pidfd_wait_retries_interruption_and_empty_returns_with_remaining_budget'),
 (unit[1],'runtime::fence_tests::pidfd_wait_expiry_or_permanent_error_does_not_retry_or_claim_readiness'),
 (unit[1],'runtime::fence_tests::pidfd_wait_timeout_does_not_claim_a_live_child_has_exited'),
 (unit[1],'runtime::fence_tests::pidfd_wait_accepts_readiness_even_with_an_expired_deadline'),
 (unit[1],'runtime::fence_tests::stale_start_time_cannot_signal_a_reused_pid'),
 (unit[1],'runtime::fence_tests::unrepresentable_fence_deadline_preserves_error_after_signaling_owned_child'),
 (integration[1],'fence_contracts::reconstructed_fence_reaps_worker_after_original_runtime_is_dropped'),
 (integration[1],'fence_contracts::externally_reaped_worker_can_still_be_fenced_by_original_runtime'),
 (integration[1],'launch_contracts::expired_startup_write_poisoning_requires_reconstruction_and_fences_worker'),
]
with Path(args.output).open('w') as output:
 for iteration in range(args.runs):
  for binary,test in cases:
   command=[binary,'--exact',test,'--test-threads=1']
   p=subprocess.run(command,capture_output=True,text=True,timeout=30,env={**os.environ,'TMPDIR':'/home/ac/projects/penelope/target/qualification-temp'})
   record={'iteration':iteration,'command':command,'returncode':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
   output.write(json.dumps(record)+'\n');output.flush()
   assert p.returncode==0,record
   assert 'test result: ok. 1 passed; 0 failed;' in p.stdout,record
print('Verified',args.runs*len(cases),'test executions in isolated processes')
