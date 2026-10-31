from pathlib import Path
import json,re,collections
root=Path(__file__).resolve().parent
cases={'duplicate-admission':2,'admission-commit':2,'duplicate-commit':2,'conflicting-admission':1,'shared-input-across-definitions':1,'shared-source-across-definitions':1,'consumed-input-at-next-frontier':1,'reused-source-at-next-frontier':1}
models={case:[0,0] for case in cases}
for compiler in ['stable','1.85.0']:
 for repeat in range(10):
  log=(root/f'repeat-{compiler}-{repeat:02}.log').read_text();assert 'test result: ok. 2 passed; 0 failed' in log and 'test result: FAILED' not in log
  starts=re.findall(r'case=(\S+) admitted=2 accepted=(\d+)$',log,re.M)
  ends=re.findall(r'case=(\S+) admitted=2 accepted=(\d+) serial_model=([01]) exact_state=true reopened_retry=true$',log,re.M)
  assert len(starts)==len(ends)==8 and dict((case,int(n)) for case,n in starts)==cases
  assert collections.Counter(case for case,n,model in ends)==collections.Counter(cases.keys())
  for case,n,model in ends:assert int(n)==cases[case];models[case][int(model)]+=1
summary={'processes':20,'test_executions':40,'gated_races':160,'admitted_requests':320,'successful_results':220,'conflict_results':100,'serial_reference_states':320,'reopened_duplicate_results':220,'reopened_conflict_results':100,'first_matching_serial_models':models}
(root/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary,indent=2))
