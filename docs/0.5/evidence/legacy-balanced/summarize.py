"""Validate and summarize every profile from a complete paired legacy campaign."""
import argparse
import json
import math
from pathlib import Path
import runpy
from statistics import median


def review_threshold(metric):
    if metric == 'p95_ns':
        return 10
    if metric in ['p50_ns', 'ns_per_operation']:
        return 5
    return None


def summarize(directory):
    metadata = json.loads((directory / 'metadata.json').read_text())
    record_count = len(list(directory.glob('run-*.json')))
    assert record_count == metadata['runs'] * 2 * len(metadata['benchmarks'])
    assert metadata['sources']['before'] == metadata['sources']['after']
    rows = []
    for benchmark in metadata['benchmarks']:
        samples = {}
        for label in ['before', 'after']:
            samples[label] = []
            for run in range(1, metadata['runs'] + 1):
                path = directory / f'run-{run}-{label}-{benchmark}.json'
                record = json.loads(path.read_text())
                assert record['revision'] == metadata['revisions'][label]
                assert record['benchmark'] == benchmark
                samples[label].append(record['observations'])
        first = samples['before'][0]
        for index, observation in enumerate(first):
            dimensions = {key: observation[key] for key in
                          ['operation', 'state_words', 'steps', 'samples', 'operations']
                          if key in observation}
            metrics = (['p50_ns', 'p95_ns', 'p99_ns']
                       if benchmark == 'timeline_operations' else ['ns_per_operation'])
            for metric in metrics:
                values = {}
                for label in ['before', 'after']:
                    values[label] = []
                    for sample in samples[label]:
                        assert len(sample) == len(first)
                        assert {key: sample[index][key] for key in dimensions} == dimensions
                        value = sample[index][metric]
                        assert math.isfinite(value) and value > 0
                        values[label].append(value)
                before, after = values['before'], values['after']
                delta = (median(after) / median(before) - 1) * 100
                threshold = review_threshold(metric)
                exceedances = (sum(right > left * (1 + threshold / 100)
                                   for left, right in zip(before, after))
                               if threshold is not None else None)
                rows.append({
                    'benchmark': benchmark, **dimensions, 'metric': metric,
                    'before': before, 'after': after,
                    'median_before': median(before), 'median_after': median(after),
                    'median_delta_percent': delta,
                    'absolute_delta_ns': median(after) - median(before),
                    'review_threshold_percent': threshold,
                    'review_flag': threshold is not None and delta > threshold,
                    'paired_threshold_exceedances': exceedances,
                })
    root = Path(__file__).resolve().parents[4]
    statistics = runpy.run_path(str(root / 'scripts/summarize-decision-perf.py'))
    for row in rows:
        if row['review_flag']:
            row['exploratory_paired_statistics'] = statistics['paired_statistics'](
                row['before'], row['after'], 10000, 3030)
    return {
        'revisions': metadata['revisions'], 'pairs': metadata['runs'],
        'records': record_count, 'source_fixtures_identical': True, 'rows': rows,
        'policy': 'Retain every observation. Thresholds are review flags, not '
                  'equivalence or statistical acceptance tests. p99 reported '
                  'without a new gate. No baseline subtraction, noise '
                  'normalization, outlier removal, or changing workloads.',
    }


if __name__ == '__main__':
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument('directory', type=Path)
    args = cli.parse_args()
    print(json.dumps(summarize(args.directory), indent=2))
