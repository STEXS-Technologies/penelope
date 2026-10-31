"""Verify complete campaigns, source/binary pins, checksums and all 39 metrics."""
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
for name, digest in manifest.items(): assert sha((root/name).read_bytes()) == digest, name
original = load('original-claim_history.rs.gz')
current = load('claim_history.rs.gz')
normalized = current
for name in ['open_started', 'commit_started', 'schedule_started', 'claim_started', 'ack_started', 'idle_started']:
    normalized = normalized.replace(name.encode(), b'started')
assert normalized == original
pre = load('pre-index-schema.rs.gz'); indexed = load('indexed-schema.rs.gz'); cached = load('rejected-cached-schema.rs.gz')
assert indexed.replace(b'.execute(sql, params)', b'.prepare_cached(sql)\n        .map_err(|error| sql_error(&error))?\n        .execute(params)', 1) == cached
campaigns = [('index-latency', pre, indexed, original, 4), ('final-index-latency', pre, cached, current, 4), ('write-cache-latency', indexed, cached, current, 5)]
for prefix, baseline, candidate, benchmark, baseline_version in campaigns:
    build = parsed(prefix+'-build.json'); metadata = parsed(prefix+'-metadata.json')
    assert all(metadata[k] == v for k,v in build.items())
    for variant, source in [('baseline', baseline), ('candidate', candidate)]:
        assert sha(source) == build['variants'][variant]['schema_sha256']
        assert 'Finished' in load(prefix+'-build-'+variant+'.log.gz').decode()
    assert sha(benchmark) == build['benchmark_sha256']['claim_history']
    assert sha(load('lease_batch.rs.gz')) == build['benchmark_sha256']['lease_batch']
    rows = parsed(prefix+'-processes.json.gz'); summary = parsed(prefix+'-summary.json')
    assert len(rows) == 80
    assert {(r['pair'],r['variant'],r['kind']) for r in rows} == {(p,v,k) for p in range(20) for v in ['baseline','candidate'] for k in ['batch','history']}
    assert {r['cpu'] for r in rows} == {metadata['cpu']}
    values = {}
    for pair in range(20):
        group = rows[pair*4:pair*4+4]
        order = ['baseline','candidate'] if pair%2 == 0 else ['candidate','baseline']
        assert [(r['variant'],r['kind']) for r in group] == [(v,k) for v in order for k in ['batch','history']]
    for row in rows:
        assert row['binary_sha256'] == build['variants'][row['variant']]['binaries'][row['kind']]['sha256']
        assert not row['stderr']
        if row['kind'] == 'batch':
            assert len(row['stdout'].splitlines()) == 4
            for line in row['stdout'].splitlines():
                m = re.fullmatch(r'timers=(false|true) batch=(1|16) iterations=400 elapsed_ns=(\d+)',line); assert m
                values[f'batch/{m[1]}/{m[2]}',row['pair'],row['variant']] = int(m[3])/400
        else:
            data = list(map(json.loads,row['stdout'].splitlines()))
            assert [d['timers'] for d in data] == [False,True]
            for d in data:
                assert d['schema_version'] == (baseline_version if row['variant'] == 'baseline' else 5)
                assert d['retained_rows'] == 1024 and d['idle_calls'] == 1000 and d['warmup_calls'] == 100
                assert d['next_token'] == (1 if d['timers'] else 1025)
                assert d['commit_calls'] == 8
                assert d['schedule_calls'] == (1024 if d['timers'] else 0)
                assert d['seed_claim_calls'] == (0 if d['timers'] else 8)
                assert d['ack_calls'] == (0 if d['timers'] else 1024)
                metrics = [('open',1),('commit',8),('idle',1000)]+([('schedule',1024)] if d['timers'] else [('seed_claim',8),('ack',1024)])
                for metric,count in metrics:
                    values[f'history/{str(d["timers"]).lower()}/{metric}',row['pair'],row['variant']] = d[metric+'_ns']/count
    assert len(summary) == 13 and set(summary) == {k[0] for k in values}
    for metric, expected in summary.items():
        ratios = [values[metric,i,'candidate']/values[metric,i,'baseline'] for i in range(20)]
        rng = random.Random(42)
        boot = sorted(statistics.mean(rng.choices(ratios,k=20)) for _ in range(10000))
        assert math.isclose(100*(statistics.mean(ratios)-1),expected['mean_paired_change_pct'],abs_tol=1e-12)
        assert all(math.isclose(100*(boot[i]-1),v,abs_tol=1e-12) for i,v in zip([250,9749],expected['bootstrap95_pct']))
        for variant in ['baseline','candidate']:
            assert math.isclose(statistics.mean(values[metric,i,variant] for i in range(20)),expected['mean_ns_per_call'][variant],abs_tol=1e-9)
    print(prefix, '80 processes / 13 metrics verified')
for compiler in ['workspace','msrv']:
    log = load(compiler+'.log.gz').decode()
    for count in [21,39,26]: assert f'test result: ok. {count} passed;' in log
    assert 'test result: FAILED' not in log and 'error: test failed' not in log
assert 'could not compile' in load('write-cache-preflight-clippy.log.gz').decode()
assert 'Finished' in load('clippy.log.gz').decode()
print('Verified 240 timing processes, rejected cache sources, and both compiler suites')
