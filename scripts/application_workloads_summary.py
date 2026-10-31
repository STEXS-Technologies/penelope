"""Validate paired workflow samples and summarize explicitly scoped overhead."""
import collections
import json
import math
import random
import statistics

CASES = {"linear": 8, "graph": 4, "retry": 9, "replay": 8, "durable": 8, "durable_replay": 8}

def summarize(rows, metadata):
    expected = {(repeat, workers, work, case, round_) for repeat in range(metadata["repeats"])
                for workers in metadata["workers"] for work in metadata["work"]
                for case in CASES for round_ in range(metadata["rounds"])}
    observed = {}
    for row in rows:
        key = (row["repeat"], row["workers"], row["work"], row["case"], row["round"])
        if key not in expected or key in observed:
            raise ValueError(f"unexpected or duplicate sample: {key}")
        if (row["schema"] != 1 or row["verified"] is not True
            or row["calls_per_workflow"] != CASES[row["case"]]
            or row["iterations_per_worker"] != metadata["iterations"]
            or row["workflows"] != row["workers"] * metadata["iterations"]
            or row["direct_first"] != (row["round"] % 2 == 0)
            or any(type(row[field]) is not int or row[field] <= 0 for field in ["direct_ns", "penelope_ns"])
            or type(row["checksum"]) is not int or not 0 <= row["checksum"] < 2**64):
            raise ValueError(f"invalid sample: {key}")
        observed[key] = row
    if set(observed) != expected:
        raise ValueError(f"incomplete campaign: {len(observed)} of {len(expected)} samples")
    # Identical business-call counts must produce identical output checksums.
    checksums = collections.defaultdict(set)
    for row in rows:
        checksums[(row["workers"], row["work"], row["calls_per_workflow"])].add(row["checksum"])
    if any(len(values) != 1 for values in checksums.values()):
        raise ValueError("business output changed between equivalent profiles")
    mix = metadata["mix"]
    if set(mix) != set(CASES) or not all(isinstance(v, (int, float)) and math.isfinite(v) and v >= 0 for v in mix.values()) or sum(mix.values()) <= 0:
        raise ValueError("mix must give every case a finite nonnegative weight and have positive total")
    def profile(samples, label):
        logs = [math.log(r["penelope_ns"] / r["direct_ns"]) for r in samples]
        blocks = [statistics.mean(math.log(r["penelope_ns"] / r["direct_ns"]) for r in samples if r["repeat"] == repeat)
                  for repeat in range(metadata["repeats"])]
        rng = random.Random(f"workflow-v1-{label}")
        bootstrap = sorted(statistics.mean(rng.choices(blocks, k=len(blocks))) for _ in range(20000))
        direct = [r["direct_ns"] / r["workflows"] for r in samples]
        managed = [r["penelope_ns"] / r["workflows"] for r in samples]
        return {"samples": len(samples), "process_blocks": len(blocks),
                "direct_median_ns_per_workflow": statistics.median(direct),
                "penelope_median_ns_per_workflow": statistics.median(managed),
                "paired_added_median_ns_per_workflow": statistics.median(b-a for a,b in zip(direct,managed)),
                "overhead_percent": 100 * math.expm1(statistics.mean(logs)),
                "process_bootstrap_95_percent": [100 * math.expm1(bootstrap[499]), 100 * math.expm1(bootstrap[19499])],
                "penelope_median_workflows_per_second": 1e9/statistics.median(managed)}
    output = {"schema": 1, "profiles": [], "weighted_estimates": [], "mix": mix}
    for workers in metadata["workers"]:
        for work in metadata["work"]:
            for case in CASES:
                samples = [r for r in rows if (r["workers"],r["work"],r["case"]) == (workers,work,case)]
                output["profiles"].append(dict(workers=workers,work=work,case=case,**profile(samples,(workers,work,case))))
            weighted = []
            for repeat in range(metadata["repeats"]):
                for round_ in range(metadata["rounds"]):
                    group = {case: observed[(repeat,workers,work,case,round_)] for case in CASES}
                    weighted.append({"repeat":repeat,"workflows":1, **{
                        field: sum(mix[case] * group[case][field] / group[case]["workflows"] for case in CASES) / sum(mix.values())
                        for field in ["direct_ns","penelope_ns"]}})
            output["weighted_estimates"].append(dict(workers=workers,work=work,**profile(weighted,(workers,work,"weighted"))))
    return output

def main():
    import argparse
    from pathlib import Path
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    metadata = json.loads((args.directory / "metadata.json").read_text())
    rows = [json.loads(line) for line in (args.directory / "samples.jsonl").read_text().splitlines()]
    result = summarize(rows, metadata)
    (args.directory / "summary.json").write_text(json.dumps(result, indent=2)+"\n")
    for row in result["profiles"]:
        print(f"{row['workers']} workers work={row['work']} {row['case']}: {row['overhead_percent']:+.3f}% overhead, {row['penelope_median_ns_per_workflow']/1000:.3f} us/workflow")
    for row in result["weighted_estimates"]:
        print(f"WEIGHTED ESTIMATE {row['workers']} workers work={row['work']}: {row['overhead_percent']:+.3f}% (declared synthetic mix only)")

if __name__ == "__main__":
    main()
