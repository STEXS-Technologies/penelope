"""Verify exact comparison reordering, complete paired data and qualification."""
from pathlib import Path
import gzip, hashlib, json, math, random, re, statistics
root = Path(__file__).resolve().parent
def load(name):
    data = (root/name).read_bytes()
    return gzip.decompress(data) if name.endswith('.gz') else data
def parsed(name): return json.loads(load(name))
def sha(data): return hashlib.sha256(data).hexdigest()
manifest = parsed('sha256.json')
assert set(manifest) == {p.name for p in root.iterdir() if p.is_file() and p.name != 'sha256.json'}
for name,digest in manifest.items(): assert sha((root/name).read_bytes()) == digest, name
def body(source,marker):
    start = source.index('{',source.index(marker));depth = 0
    for index in range(start,len(source)):
        if source[index] == '{': depth += 1
        elif source[index] == '}':
            depth -= 1
            if depth == 0: return source[start:index+1]
    raise AssertionError('unclosed function')
baseline = load('baseline-implementation.rs.gz').decode(); candidate = load('candidate-implementation.rs.gz').decode()
assert candidate.replace(body(candidate,'fn same_effect_key('),body(baseline,'fn same_effect_key('),1) == baseline
reference = body(load('legacy_atomic_validator.rs.gz').decode(),'pub fn validate(')
original = body(load('oracle-parent-implementation.rs.gz').decode(),'pub fn validate(&self) -> Result<(), CommitValidationError>')
assert re.sub(r'\s','',reference) == re.sub(r'\s','',re.sub(r'\bself\b','commit',original))
build = parsed('build.json'); metadata = parsed('metadata.json')
assert all(metadata[k] == v for k,v in build.items())
for variant,source in [('baseline',baseline),('candidate',candidate)]:
    assert sha(source.encode()) == build['variants'][variant]['source_sha256']
    for package in ['penelope-ports','penelope-sqlite']:
        assert 'Finished' in load('build-'+variant+'-'+package+'.log.gz').decode()
for path,digest in build['fixture_sha256'].items(): assert sha(load(Path(path).name+'.gz')) == digest
rows = parsed('processes.json.gz');summary = parsed('summary.json')
profiles = ['short-step','short-payload','long-step','long-payload']
requests = [(profile,kind) for profile in profiles for kind in ['allocations','cpu']]+[('short-step','history')]
assert len(rows) == 360
assert {(r['pair'],r['variant'],r['profile'],r['kind']) for r in rows} == {(p,v,profile,kind) for p in range(20) for v in ['baseline','candidate'] for profile,kind in requests}
assert {r['cpu'] for r in rows} == {metadata['cpu']}
for pair in range(20):
    order = ['baseline','candidate'] if pair%2 == 0 else ['candidate','baseline']
    assert [(r['variant'],r['profile'],r['kind']) for r in rows[pair*18:(pair+1)*18]] == [(v,profile,kind) for v in order for profile,kind in requests]
values = {}
for row in rows:
    binary = {'allocations':'atomic_validation_allocations','cpu':'atomic_validation','history':'claim_history'}[row['kind']]
    assert row['binary_sha256'] == build['variants'][row['variant']]['binaries'][binary]['sha256']
    assert not row['stderr']
    lines = row['stdout'].splitlines()
    if row['kind'] in ['allocations','cpu']:
        assert len(lines) == 4
        for line,actions in zip(lines,[0,1,16,128]):
            if row['kind'] == 'allocations':
                m = re.fullmatch(r'actions=(\d+) allocations=(\d+) reallocations=(\d+) deallocations=(\d+) requested_bytes=(\d+)',line); assert m
                count = 8 if row['profile'].startswith('long') else 0
                assert list(map(int,m.groups())) == [actions,count,0,count,count*128]
            else:
                m = re.fullmatch(r'actions=(\d+) iterations=(\d+) elapsed_ns=(\d+)',line); assert m
                iterations = {0:100000,1:100000,16:10000,128:1000}[actions]//(10 if row['profile'].startswith('long') else 1)
                assert int(m[1]) == actions and int(m[2]) == iterations
                values[f'cpu/{row["profile"]}/{actions}',row['pair'],row['variant']] = int(m[3])/iterations
    else:
        data = list(map(json.loads,lines)); assert [d['timers'] for d in data] == [False,True]
        for d in data:
            assert d['schema_version'] == 5 and d['retained_rows'] == 1024
            assert d['next_token'] == (1 if d['timers'] else 1025)
            assert d['commit_calls'] == 8 and d['idle_calls'] == 1000 and d['warmup_calls'] == 100
            assert d['schedule_calls'] == (1024 if d['timers'] else 0)
            assert d['seed_claim_calls'] == (0 if d['timers'] else 8)
            assert d['ack_calls'] == (0 if d['timers'] else 1024)
            for metric,count in [('open',1),('commit',8),('idle',1000)]+([('schedule',1024)] if d['timers'] else [('seed_claim',8),('ack',1024)]):
                values[f'history/{str(d["timers"]).lower()}/{metric}',row['pair'],row['variant']] = d[metric+'_ns']/count
assert len(summary) == 25 and set(summary) == {key[0] for key in values}
for metric,d in summary.items():
    ratios = [values[metric,i,'candidate']/values[metric,i,'baseline'] for i in range(20)]
    rng = random.Random(42)
    boot = sorted(statistics.mean(rng.choices(ratios,k=20)) for _ in range(10000))
    assert math.isclose(100*(statistics.mean(ratios)-1),d['mean_paired_change_pct'],abs_tol=1e-12)
    assert all(math.isclose(100*(boot[i]-1),v,abs_tol=1e-12) for i,v in zip([250,9749],d['bootstrap95_pct']))
    for variant in ['baseline','candidate']:
        assert math.isclose(statistics.mean(values[metric,i,variant] for i in range(20)),d['mean_ns_per_call'][variant],abs_tol=1e-9)
confirmation_metadata = parsed('confirmation-metadata.json')
assert all(confirmation_metadata[k] == v for k,v in build.items())
confirmation_rows = parsed('confirmation-processes.json.gz')
confirmation_summary = parsed('confirmation-summary.json')
assert len(confirmation_rows) == 40
assert {(r['pair'],r['variant']) for r in confirmation_rows} == {(p,v) for p in range(20) for v in ['baseline','candidate']}
assert {r['cpu'] for r in confirmation_rows} == {confirmation_metadata['cpu']}
confirmation_values = {}
for pair in range(20):
    assert [r['variant'] for r in confirmation_rows[pair*2:pair*2+2]] == (['baseline','candidate'] if pair%2 == 0 else ['candidate','baseline'])
for row in confirmation_rows:
    assert row['kind'] == 'history' and row['profile'] == 'short-step' and not row['stderr']
    assert row['binary_sha256'] == build['variants'][row['variant']]['binaries']['claim_history']['sha256']
    data = list(map(json.loads,row['stdout'].splitlines()))
    assert [d['timers'] for d in data] == [False,True]
    for d in data:
        assert d['schema_version'] == 5 and d['retained_rows'] == 1024
        assert d['next_token'] == (1 if d['timers'] else 1025)
        assert d['commit_calls'] == 8 and d['idle_calls'] == 1000 and d['warmup_calls'] == 100
        assert d['schedule_calls'] == (1024 if d['timers'] else 0)
        assert d['seed_claim_calls'] == (0 if d['timers'] else 8)
        assert d['ack_calls'] == (0 if d['timers'] else 1024)
        for metric,count in [('open',1),('commit',8),('idle',1000)]+([('schedule',1024)] if d['timers'] else [('seed_claim',8),('ack',1024)]):
            confirmation_values[f'history/{str(d["timers"]).lower()}/{metric}',row['pair'],row['variant']] = d[metric+'_ns']/count
assert len(confirmation_summary) == 9 and set(confirmation_summary) == {key[0] for key in confirmation_values}
for metric,d in confirmation_summary.items():
    ratios = [confirmation_values[metric,i,'candidate']/confirmation_values[metric,i,'baseline'] for i in range(20)]
    rng = random.Random(42)
    boot = sorted(statistics.mean(rng.choices(ratios,k=20)) for _ in range(10000))
    assert math.isclose(100*(statistics.mean(ratios)-1),d['mean_paired_change_pct'],abs_tol=1e-12)
    assert all(math.isclose(100*(boot[i]-1),v,abs_tol=1e-12) for i,v in zip([250,9749],d['bootstrap95_pct']))
    for variant in ['baseline','candidate']:
        assert math.isclose(statistics.mean(confirmation_values[metric,i,variant] for i in range(20)),d['mean_ns_per_call'][variant],abs_tol=1e-9)
rows = parsed('repeated.json.gz')
assert len(rows) == 50
assert {(r['compiler'],r['repeat']) for r in rows} == {(c,i) for c in ['stable','1.85.0'] for i in range(25)}
for row in rows:
    assert row['returncode'] == 0 and not row['stderr'] and 'test result: ok. 2 passed; 0 failed;' in row['stdout']
for compiler in ['stable','1.85.0']:
    assert len({r['binary_sha256'] for r in rows if r['compiler'] == compiler}) == 1
for filename in ['workspace.log.gz','msrv.log.gz']:
    log = load(filename).decode()
    for marker,count in [(r'Running unittests src/lib.rs \([^\n]*penelope_sqlite-',23),(r'Running tests/process.rs \(',40),(r'Running tests/tasks.rs \(',26)]:
        section = re.split(marker,log,maxsplit=1)[1].split('     Running ',1)[0]
        assert f'test result: ok. {count} passed;' in section
    assert 'test result: FAILED' not in log and 'error: test failed' not in log
for filename in ['ports-final.log.gz','ports-msrv-final.log.gz']:
    log = load(filename).decode()
    assert 'test result: ok. 38 passed;' in log
    section = log.split('Running tests/atomic_validation_parity.rs',1)[1].split('     Running ',1)[0]
    assert 'test result: ok. 2 passed; 0 failed;' in section
assert 'Finished' in load('clippy-final.log.gz').decode()
assert not load('fmt-final.log.gz')
assert 'Code graph updated.' in load('graph-final.log.gz').decode()
print('Verified only comparison ordering changed, 400 measurement processes / 34 metrics, unchanged allocations, original oracle and 50 parity processes')
