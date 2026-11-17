#!/usr/bin/env python3
"""Summarize retained post-QPSG 0.4-versus-0.5 durable-store pairs."""
import json
import math
from pathlib import Path
import random
import statistics
import sys


PAIRS = 31
BOOTSTRAPS = 5000
SEED = 1


def leaves(value, path=""):
    if isinstance(value, dict):
        for key, child in value.items():
            yield from leaves(child, path + "/" + key)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from leaves(child, path + "/" + str(index))
    elif isinstance(value, (int, float)) and path.endswith(("_ns", "_kib")):
        yield path, value


def read_run(directory):
    report = json.loads((directory / "run-1.json").read_text())
    if report.get("journal_mode") != "WAL" or report.get("synchronous") != "FULL":
        raise ValueError(f"unexpected SQLite durability profile in {directory}")
    if [task["records"] for task in report["tasks"]] != [16, 128, 512]:
        raise ValueError(f"unexpected task fixtures in {directory}")
    metrics = dict(leaves(report))
    metrics.update(leaves(json.loads((directory / "run-1.resources.json").read_text())))
    return metrics


def summarize(root):
    comparison = json.loads((root / "comparison.json").read_text())
    if comparison.get("pairs") != PAIRS or comparison.get("warmup_pairs") != 1:
        raise ValueError("comparison metadata does not describe 31 pairs plus one warmup")
    samples = {label: [] for label in ("before", "after")}
    for pair in range(1, PAIRS + 1):
        for label in samples:
            samples[label].append(read_run(root / f"pair-{pair:02d}-{label}"))
    keys = set(samples["before"][0])
    if any(set(row) != keys for group in samples.values() for row in group):
        raise ValueError("metric keys differ between benchmark runs")

    rng = random.Random(SEED)
    rows = []
    for metric in sorted(keys):
        before = [row[metric] for row in samples["before"]]
        after = [row[metric] for row in samples["after"]]
        paired = [(a / b - 1) * 100 for b, a in zip(before, after) if b]
        paired_logs = [math.log(a / b) for b, a in zip(before, after) if b]
        if len(paired) != PAIRS:
            raise ValueError(f"zero baseline measurement for {metric}")
        threshold = 5 if metric.endswith("/p50_ns") else 10 if metric.endswith("/p95_ns") else None
        bootstrap = [math.expm1(statistics.mean(rng.choices(paired_logs, k=PAIRS))) * 100
                     for _ in range(BOOTSTRAPS)]
        bootstrap.sort()
        low = max(0, math.ceil(BOOTSTRAPS * 0.025) - 1)
        high = max(0, math.ceil(BOOTSTRAPS * 0.975) - 1)
        delta = (statistics.median(after) / statistics.median(before) - 1) * 100
        rows.append({
            "metric": metric,
            "before": before,
            "after": after,
            "median_before": statistics.median(before),
            "median_after": statistics.median(after),
            "median_delta_percent": delta,
            "review_threshold_percent": threshold,
            "review_flag": threshold is not None and delta > threshold,
            "paired_threshold_exceedances": sum(value > threshold for value in paired)
                if threshold is not None else None,
            "paired_median_delta_percent": statistics.median(paired),
            "paired_geometric_mean_delta_percent": math.expm1(statistics.mean(paired_logs)) * 100,
            "paired_bootstrap_percentile_95_percent": [bootstrap[low], bootstrap[high]],
        })
    return {
        "revisions": comparison["revisions"],
        "pairs": PAIRS,
        "warmup_pairs_excluded": 1,
        "metrics": rows,
        "bootstrap": {
            "method": "paired log-ratio geometric mean; percentile bootstrap over round pairs",
            "resamples": BOOTSTRAPS,
            "seed": SEED,
            "interval": "percentile 95%; exploratory, not equivalence or acceptance",
        },
        "limits": [
            "Medians summarize run-level quantiles; they are not pooled event percentiles.",
            "A bootstrap interval does not waive a sustained performance review flag.",
            "The fixture has unchanged data and benchmark source; cache and shared-host effects remain uncontrolled.",
        ],
    }


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} COMPARISON_DIRECTORY")
    root = Path(sys.argv[1]).resolve()
    result = summarize(root)
    output = root / "summary.json"
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(output)
