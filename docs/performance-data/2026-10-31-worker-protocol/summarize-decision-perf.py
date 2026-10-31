#!/usr/bin/env python3
"""Summarize paired perf benchmark records; bootstrap intervals are descriptive."""
import argparse
import json
import math
from pathlib import Path
import random
from statistics import mean, median

METRICS = ["ns_per_operation", "task-clock", "instructions", "cycles"]


def paired_statistics(before, after, resamples=10000, seed=3030):
    if not before or len(before) != len(after) or resamples < 1:
        raise ValueError("paired positive samples and resamples are required")
    if any(not math.isfinite(value) or value <= 0 for value in before + after):
        raise ValueError("paired values must be finite and positive")
    logs = [math.log(right / left) for left, right in zip(before, after)]
    generator = random.Random(seed)
    bootstrap = sorted(math.expm1(mean(generator.choices(logs, k=len(logs)))) * 100
                       for _ in range(resamples))
    return {
        "samples_per_revision": len(logs),
        "before": {"median": median(before), "min": min(before), "max": max(before)},
        "after": {"median": median(after), "min": min(after), "max": max(after)},
        "median_delta_percent": (median(after) / median(before) - 1) * 100,
        "paired_geometric_mean_delta_percent": math.expm1(mean(logs)) * 100,
        "paired_bootstrap_percentile_95_percent": [
            bootstrap[max(0, math.ceil(resamples * 0.025) - 1)],
            bootstrap[max(0, math.ceil(resamples * 0.975) - 1)],
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--benchmark", default="parallel_linear_saga")
    parser.add_argument("--seed", type=int, default=3030)
    parser.add_argument("--resamples", type=int, default=10000)
    args = parser.parse_args()
    metadata = json.loads((args.directory / "metadata.json").read_text())
    if not metadata.get("perf") or args.benchmark not in metadata["benchmarks"]:
        parser.error("selected benchmark must have recorded perf data")
    samples = {label: [] for label in ["before", "after"]}
    for index in range(1, metadata["runs"] + 1):
        for label in samples:
            sample = json.loads((args.directory / f"run-{index}-{label}-{args.benchmark}.json").read_text())
            if (sample["revision"] != metadata["revisions"][label]
                    or sample["benchmark"] != args.benchmark or len(sample["observations"]) != 1):
                raise ValueError("sample revision, benchmark or operation count is inconsistent")
            samples[label].append(sample)
    counts = {sample["observations"][0]["operations"]
              for group in samples.values() for sample in group}
    if len(counts) != 1:
        raise ValueError("paired benchmark operation counts must agree")
    result = {"schema": 1, "benchmark": args.benchmark, "revisions": metadata["revisions"],
              "bootstrap_seed": args.seed, "bootstrap_resamples": args.resamples,
              "method": "paired log-ratio geometric mean; percentile bootstrap over round pairs",
              "limitations": "exploratory intervals; no noninferiority margin or release acceptance test",
              "metrics": {}}
    for metric in METRICS:
        def value(sample):
            if metric == "ns_per_operation":
                return sample["observations"][0][metric]
            return float(next(counter["counter-value"] for counter in sample["perf"]
                              if counter["event"] == metric))
        result["metrics"][metric] = paired_statistics(
            [value(sample) for sample in samples["before"]],
            [value(sample) for sample in samples["after"]], args.resamples, args.seed)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
