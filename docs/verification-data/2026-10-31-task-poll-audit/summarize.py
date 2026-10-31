"""Recalculate exploratory pilot results from retained process stdout."""
import gzip
import json
from pathlib import Path
import re
import statistics

ROOT = Path(__file__).resolve().parent
CAMPAIGNS = {
    'factored': ('task-poll-pilot.json.gz', 'candidate'),
    'inline': ('task-poll-inline-pilot.json.gz', 'inline'),
    'pending-equality': ('task-poll-pending-pilot.json.gz', 'pending'),
    'pending-match': ('task-poll-match-pilot.json.gz', 'match'),
}
summary = {}
for name, (filename, candidate) in CAMPAIGNS.items():
    records = json.loads(gzip.decompress((ROOT / filename).read_bytes()))
    assert len(records) == 16
    values = {}
    for index, record in enumerate(records):
        pair = index // 2
        order = ['baseline', candidate] if pair % 2 == 0 else [candidate, 'baseline']
        assert record['pair'] == pair and record['label'] == order[index % 2]
        lines = record['stdout'].splitlines()
        assert len(lines) == 9
        for line in lines:
            match = re.fullmatch(
                r'(.+): (\d+) operations in ([\d.]+)(ns|µs|ms|s) \(\d+ ns/op\)', line)
            assert match, line
            operation, count, duration, unit = match.groups()
            expected = 20_000 if operation == 'hosted pending capacity=65536' else 2_000_000
            assert int(count) == expected
            ns = float(duration) * {'ns': 1, 'µs': 1e3, 'ms': 1e6, 's': 1e9}[unit]
            key = pair, record['label']
            group = values.setdefault(operation, {})
            assert key not in group
            group[key] = ns / int(count)
    summary[name] = {}
    for operation, group in values.items():
        baseline = [group[pair, 'baseline'] for pair in range(8)]
        changed = [group[pair, candidate] for pair in range(8)]
        differences = [100 * (c / b - 1) for b, c in zip(baseline, changed)]
        summary[name][operation] = {
            'baseline_median_ns': statistics.median(baseline),
            'candidate_median_ns': statistics.median(changed),
            'paired_median_percent': statistics.median(differences),
            'all_paired_percent': differences,
        }
print(json.dumps(summary, indent=2))
