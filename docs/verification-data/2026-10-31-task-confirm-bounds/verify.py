"""Validate archived generation-bound and retirement regression evidence."""
from pathlib import Path
import gzip,hashlib,itertools,json,re
root=Path(__file__).resolve().parent
manifest=json.loads((root/'manifest.json').read_text())
for name,digest in manifest['sha256'].items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name

def read(name):
 p=root/name
 return p.read_text() if p.exists() else gzip.decompress(p.with_name(p.name+'.gz').read_bytes()).decode()
for compiler in ['stable','1.85.0']:
 for repeat in range(10):
  for suite,n in [('confirm',2),('columns',2),('orphans',1)]:
   log=read(f'repeat-{compiler}-{repeat:02}-{suite}.log');assert f'test result: ok. {n} passed; 0 failed' in log
   if suite=='confirm':
    cases=re.findall(r'generation=(\d+) state=(\d+) reserved=(true|false) rejected_unchanged=true reopened=true',log)
    assert len(cases)==12 and set(cases)==set(itertools.product(['0','4','4294967295'],['1','2'],['true','false']))
    valid=re.findall(r'reserved=(true|false) last_attempt=true idempotent=true retired_reopened=true wrong_identity_unchanged=true',log)
    assert sorted(valid)==['false','true']
   if suite=='columns':assert log.count('reserve=')==27
   if suite=='orphans':
    cases=re.findall(r'reused_input=(true|false) cold=(true|false) bound=(\d+) retained=(\d+) rejection_atomic=(true|false) exact_usage=true repaired_reopened=true',log)
    assert len(cases)==12
    assert {(reuse,cold,int(bound)-int(retained)) for reuse,cold,bound,retained,_ in cases}==set(itertools.product(['true','false'],['true','false'],[-1,0,1]))
    assert sum(rejected=='true' for *_,rejected in cases)==4
    for reuse,cold,bound,retained,rejected in cases:assert (rejected=='true')==(int(bound)<int(retained))
 for n in [27,51,42]:assert f'test result: ok. {n} passed; 0 failed' in read(f'final-{compiler}-sqlite.log')
control=read('baseline-control.log');assert 'left: Ok(())' in control and 'right: Err(Conflict)' in control and 'test result: FAILED. 1 passed; 1 failed' in control
assert 'valid_confirmations_survive_last_attempt_retirement_and_reopen ... ok' in control
for name in ['workspace.log','msrv.log','clippy.log']:
 log=read(name);assert 'error:' not in log and 'test result: FAILED' not in log
for n in [27,51,42]:assert f'test result: ok. {n} passed; 0 failed' in read('msrv.log')
assert 'Code graph updated' in read('graphify.log')
base=read('baseline-authority.rs').split('fn confirm_stopped(',1)[1]
fixed=read('fixed-authority.rs').split('fn confirm_stopped(',1)[1]
assert 'generation == TaskGeneration::ZERO' in fixed and 'generation.get() > self.definition.recovery.max_attempts' in fixed
assert 'generation == TaskGeneration::ZERO' not in base
assert fixed.split('let changed =',1)[1]==base.split('let changed =',1)[1]
ci=json.loads(read('previous-ci.json'));assert ci['conclusion']=='success' and ci['status']=='completed' and ci['headSha']==manifest['baseline_commit'] and all(job['conclusion']=='success' for job in ci['jobs'])
print('Verified baseline counterexample, 60 regression processes, 240 invalid-generation fixtures, 40 valid retirement workflows, 540 identity fixtures, 240 quota fixtures, final qualification and archive hashes.')
