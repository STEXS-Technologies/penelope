#!/usr/bin/env python3
"""Reopen an actual 0.4-writer database and preserve its rows and replay output."""
import gzip
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile


def snapshot(path, tables=None):
    with sqlite3.connect(path) as connection:
        if tables is None:
            tables = [row[0] for row in connection.execute(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
            )]
        result = {}
        for table in sorted(tables):
            quoted = '"' + table.replace('"', '""') + '"'
            rows = connection.execute(f"SELECT * FROM {quoted}").fetchall()
            encoded = [[{"type": type(cell).__name__,
                         "value": cell.hex() if isinstance(cell, bytes) else cell}
                        for cell in row] for row in rows]
            schema = connection.execute(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?", (table,)
            ).fetchone()[0]
            result[table] = {"schema": schema, "rows": sorted(encoded, key=json.dumps)}
        return result


def main():
    repository = Path(__file__).resolve().parents[1]
    fixture = repository / "ci/sqlite-upgrade/baseline"
    manifest = json.loads((fixture / "manifest.json").read_text())
    database = gzip.decompress((fixture / "database.sqlite.gz").read_bytes())
    assert hashlib.sha256(database).hexdigest() == manifest["database_sha256"]
    expected = (fixture / "replay.txt").read_bytes()
    assert hashlib.sha256(expected).hexdigest() == manifest["replay_sha256"]
    source = repository / "ci/sqlite-upgrade/main.rs"
    assert hashlib.sha256(source.read_bytes()).hexdigest() == manifest["producer_source_sha256"]
    with tempfile.TemporaryDirectory(prefix="penelope-sqlite-upgrade-") as temporary:
        path = Path(temporary) / "database.sqlite"
        path.write_bytes(database)
        before = snapshot(path)
        with sqlite3.connect(path) as connection:
            assert connection.execute("PRAGMA user_version").fetchone()[0] == 6
        for attempt in range(2):
            result = subprocess.run([
                "cargo", "run", "--manifest-path", str(source.parent / "Cargo.toml"),
                "--locked", "--quiet", "--", "read", str(path),
            ], cwd=repository, check=True, capture_output=True)
            assert result.stdout == expected, "legacy public replay changed"
            assert snapshot(path, before) == before, "legacy rows or table schemas changed"
            with sqlite3.connect(path) as connection:
                assert connection.execute("PRAGMA user_version").fetchone()[0] == 9
                assert connection.execute("PRAGMA integrity_check").fetchall() == [("ok",)]
            print(f"Upgrade/reopen {attempt + 1}: exact legacy rows, replay and integrity retained")
        # Restore is a separate, quiesced authority, never a schema downgrade.
        restored = Path(temporary) / "restored.sqlite"
        restored.write_bytes(database)
        assert snapshot(restored) == before
    print(f"Actual 0.4 writer fixture: {len(before)} legacy tables verified")


if __name__ == "__main__":
    main()
