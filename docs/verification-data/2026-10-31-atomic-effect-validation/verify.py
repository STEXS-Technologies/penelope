"""Verify paired timings, allocation counts, oracle parity and source pins."""
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
baseline = load('baseline-implementation.rs.gz'); candidate = load('candidate-implementation.rs.gz')
def body(source,marker):
    start = source.index(marker); start = source.index('{',start)
    depth = 0
    for index in range(start,len(source)):
        if source[index] == '{': depth += 1
        elif source[index] == '}':
            depth -= 1
            if depth == 0: return source[start:index+1]
    raise AssertionError('unclosed function')
reference = body(load('legacy_atomic_validator.rs.gz').decode(),'pub fn validate(')
original = body(baseline.decode(),'pub fn validate(&self) -> Result<(), CommitValidationError>')
assert re.sub(r'\s','',reference) == re.sub(r'\s','',re.sub(r'\bself\b','commit',original))
for prefix,long in [('atomic-validation',False),('atomic-validation-long',True)]:
    build = parsed(prefix+'-build.json'); metadata = parsed(prefix+'-metadata.json')
    assert all(metadata[k] == v for k,v in build.items())
    for variant,source in [('baseline',baseline),('candidate',candidate)]:
        assert sha(source) == build['variants'][variant]['source_sha256']
        for package in (['penelope-ports'] if long else ['penelope-ports','penelope-sqlite']):
            assert 'Finished' in load(prefix+'-build-'+variant+'-'+package+'.log.gz').decode()
    for path,digest in build['fixture_sha256'].items():
        name = Path(path).name
        archived = (('short-' if not long and name != 'claim_history.rs' else '')+name+'.gz')
        assert sha(load(archived)) == digest, archived
    rows = parsed(prefix+'-processes.json.gz'); summary = parsed(prefix+'-summary.json')
    kinds = ['allocations','cpu']+([] if long else ['history'])
    assert len(rows) == 40*len(kinds)
    assert {(r['pair'],r['variant'],r['kind']) for r in rows} == {(p,v,k) for p in range(20) for v in ['baseline','candidate'] for k in kinds}
    assert {r['cpu'] for r in rows} == {metadata['cpu']}
    group_size = 2*len(kinds)
    for pair in range(20):
        group = rows[pair*group_size:(pair+1)*group_size]
        order = ['baseline','candidate'] if pair%2 == 0 else ['candidate','baseline']
        assert [(r['variant'],r['kind']) for r in group] == [(v,k) for v in order for k in kinds]
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
                    count = (8+(5*actions*(actions+1)//2 if row['variant']=='baseline' else 0)) if long else 0
                    assert list(map(int,m.groups())) == [actions,count,0,count,count*128]
                else:
                    m = re.fullmatch(r'actions=(\d+) iterations=(\d+) elapsed_ns=(\d+)',line); assert m
                    iterations = {0:100000,1:100000,16:10000,128:1000}[actions]//(10 if long else 1)
                    assert int(m[1]) == actions and int(m[2]) == iterations
                    values[f'cpu/{actions}',row['pair'],row['variant']] = int(m[3])/iterations
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
    assert len(summary) == (4 if long else 13) and set(summary) == {key[0] for key in values}
    for metric,d in summary.items():
        ratios = [values[metric,i,'candidate']/values[metric,i,'baseline'] for i in range(20)]
        rng = random.Random(42)
        boot = sorted(statistics.mean(rng.choices(ratios,k=20)) for _ in range(10000))
        assert math.isclose(100*(statistics.mean(ratios)-1),d['mean_paired_change_pct'],abs_tol=1e-12)
        assert all(math.isclose(100*(boot[i]-1),v,abs_tol=1e-12) for i,v in zip([250,9749],d['bootstrap95_pct']))
        for variant in ['baseline','candidate']:
            assert math.isclose(statistics.mean(values[metric,i,variant] for i in range(20)),d['mean_ns_per_call'][variant],abs_tol=1e-9)
    print(prefix,len(rows),'processes /',len(summary),'metrics verified')
rows = parsed('repeated.json.gz')
assert len(rows) == 50
assert {(r['compiler'],r['repeat']) for r in rows} == {(c,i) for c in ['stable','1.85.0'] for i in range(25)}
for row in rows:
    assert row['returncode'] == 0 and not row['stderr'] and 'test result: ok. 1 passed; 0 failed;' in row['stdout']
for compiler in ['stable','1.85.0']:
    assert len({r['binary_sha256'] for r in rows if r['compiler'] == compiler}) == 1
for filename in ['workspace.log.gz','msrv.log.gz']:
    log = load(filename).decode()
    for marker,count in [(r'Running unittests src/lib.rs \([^\n]*penelope_sqlite-',23),(r'Running tests/process.rs \(',40),(r'Running tests/tasks.rs \(',26)]:
        section = re.split(marker,log,maxsplit=1)[1].split('     Running ',1)[0]
        assert f'test result: ok. {count} passed;' in section
    assert 'test result: FAILED' not in log and 'error: test failed' not in log
for filename in ['ports-extended.log.gz','ports-msrv-extended.log.gz']:
    log = load(filename).decode()
    assert 'test result: ok. 38 passed;' in log
    section = log.split('Running tests/atomic_validation_parity.rs',1)[1].split('     Running ',1)[0]
    assert 'test result: ok. 1 passed; 0 failed;' in section
assert 'Finished' in load('clippy-extended.log.gz').decode()
assert 'could not compile' in load('parity-preflight.log.gz').decode()
assert not load('fmt-extended.log.gz')
assert 'Code graph updated.' in load('graph-extended.log.gz').decode()
print('Verified 200 measurement processes, 17 metrics, exact allocations, original oracle and 50 parity processes')
