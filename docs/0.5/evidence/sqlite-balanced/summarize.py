"""Summarize all nine SQLite pairs and the three-variant attribution diagnostic."""
import argparse
import json
from pathlib import Path
from statistics import median


def leaves(value, path=''):
    if isinstance(value, dict):
        for key, child in value.items():
            yield from leaves(child, path + '/' + key)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from leaves(child, path + '/' + str(index))
    elif isinstance(value, (int, float)) and path.endswith(('_ns', '_kib')):
        yield path, value


def read(directory):
    report = json.loads((directory / 'run-1.json').read_text())
    assert report['journal_mode'] == 'WAL' and report['synchronous'] == 'FULL'
    assert [task['records'] for task in report['tasks']] == [16, 128, 512]
    metrics = dict(leaves(report))
    metrics.update(dict(leaves(json.loads((directory / 'run-1.resources.json').read_text()))))
    return metrics


def compare(directory, before, after):
    samples = {label: [read(directory / f'round-{i}-{label}') for i in range(1, 10)]
               for label in [before, after]}
    keys = set(samples[before][0])
    assert all(set(sample) == keys for group in samples.values() for sample in group)
    rows = []
    for metric in sorted(keys):
        b, a = ([sample[metric] for sample in samples[label]] for label in [before, after])
        threshold = 5 if metric.endswith('/p50_ns') else 10 if metric.endswith('/p95_ns') else None
        delta = (median(a) / median(b) - 1) * 100
        rows.append({'metric': metric, 'before': b, 'after': a,
                     'median_before': median(b), 'median_after': median(a),
                     'median_delta_percent': delta,
                     'review_threshold_percent': threshold,
                     'review_flag': threshold is not None and delta > threshold})
    return {'before': before, 'after': after, 'pairs': 9, 'rows': rows}


if __name__ == '__main__':
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument('paired', type=Path)
    cli.add_argument('diagnostic', type=Path)
    args = cli.parse_args()
    print(json.dumps({'original_comparison': compare(args.paired, 'before', 'after'),
                     'attribution': [compare(args.diagnostic, 'baseline', 'patched-baseline'),
                                     compare(args.diagnostic, 'patched-baseline', 'candidate')],
                     'limits': 'All measured observations retained; full warmup pair/triple excluded from aggregates only. Medians of run percentiles, no pooled-tail or statistical-equivalence claim. Reopened connection is not cold OS cache. Ten replay samples per dataset cannot resolve rare tails. Database-open timing is a single sample per run. External worker execution, machine journal throughput, WAL size and disk writes are not measured.'}, indent=2))
