"""Reproduce summary.json for an extracted benchmark campaign."""
import importlib.util
import json
import sys
from pathlib import Path
sys.dont_write_bytecode = True
root = Path(sys.argv[1])
helper = Path(__file__).with_name("summarize-decision-perf.py")
spec = importlib.util.spec_from_file_location("stats", helper)
stats = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stats)
metadata = json.loads((root / "metadata.json").read_text())
result = {"revisions": metadata["revisions"], "operations": {}, "whole_process_counters": {}}
for benchmark in metadata["benchmarks"]:
    records = {label: [json.loads((root / f"run-{i}-{label}-{benchmark}.json").read_text())
                      for i in range(1, metadata["runs"] + 1)] for label in ["before", "after"]}
    for label, group in records.items():
        assert all(record["revision"] == metadata["revisions"][label] for record in group)
    for index, observation in enumerate(records["before"][0]["observations"]):
        assert all(record["observations"][index]["operation"] == observation["operation"]
                   and record["observations"][index]["operations"] == observation["operations"]
                   for group in records.values() for record in group)
        result["operations"][observation["operation"]] = stats.paired_statistics(*[
            [record["observations"][index]["ns_per_operation"] for record in records[label]]
            for label in records])
    result["whole_process_counters"][benchmark] = {}
    for event in ["instructions", "cycles", "task-clock"]:
        result["whole_process_counters"][benchmark][event] = stats.paired_statistics(*[
            [float(next(counter["counter-value"] for counter in record["perf"]
                        if counter["event"] == event)) for record in records[label]]
            for label in records])
print(json.dumps(result, indent=2))
