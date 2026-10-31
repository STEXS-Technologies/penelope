"""Check the complete race matrix and summarize observed winners without filtering."""
from pathlib import Path
import json,re
out=Path(__file__).resolve().parent
expected={('bytes',name) for name in ['payload-payload-cross-definition','inbox-inbox','payload-inbox','payload-commit','payload-timer']}|{('scopes',name) for name in ['payload-payload','inbox-inbox','payload-inbox','payload-commit']}
winners={f'{kind}/{case}':[0,0] for kind,case in sorted(expected)}
for compiler in ['stable','1.85.0']:
 for repeat in range(10):
  log=(out/f'repeat-{compiler}-{repeat:02}.log').read_text()
  assert 'test result: ok. 2 passed; 0 failed' in log
  gates=re.findall(r'kind=(bytes|scopes) case=([a-z-]+) admitted=2 accepted=(\d+)',log)
  assert len(gates)==9 and {(kind,case) for kind,case,_ in gates}==expected
  assert all(accepted=='1' for _,_,accepted in gates)
  cases=re.findall(r'kind=(bytes|scopes) case=([a-z-]+) winner=([01]) exact_state=true serial_rejections=2 reopened_retry=true',log)
  assert len(cases)==9 and {(kind,case) for kind,case,_ in cases}==expected
  for kind,case,winner in cases:winners[f'{kind}/{case}'][int(winner)]+=1
summary={'processes':20,'test_executions':40,'races':180,'admitted_requests':360,'race_winners':180,'race_quota_rejections':180,'serial_model_rejections':360,'reopened_duplicate_retries':180,'reopened_loser_rejections':180,'observed_winners':winners}
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))
