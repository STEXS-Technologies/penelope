#!/usr/bin/env python3
"""Verify raw warmed replay allocation samples; no latency inference."""
import json
from pathlib import Path
root = Path(__file__).resolve().parent
expected = {
    "baseline": {"first": (9, 4, 2056), "last": (9, 4, 2056),
                 "end": (8, 4, 1000), "past": (8, 4, 1000), "page": (9, 9, 67528)},
    "candidate": {"first": (9, 4, 1264), "last": (9, 4, 1264),
                  "end": (8, 4, 1000), "past": (8, 4, 1000), "page": (9, 4, 34792)},
}
for revision, profiles in expected.items():
    rows = [json.loads(line) for line in (root / f"{revision}.jsonl").read_text().splitlines()]
    assert len(rows) == 80
    for profile, traffic in profiles.items():
        samples = [row for row in rows if row["profile"] == profile]
        assert len(samples) == 16
        assert sorted(row["sample"] for row in samples) == list(range(16))
        for row in samples:
            assert tuple(row[key] for key in ("allocations", "reallocations", "requested_bytes")) == traffic
            assert row["rows"] == (128 if profile == "page" else 0 if profile in ("end", "past") else 1)
print("Verified all 160 replay allocation samples across five profiles.")
