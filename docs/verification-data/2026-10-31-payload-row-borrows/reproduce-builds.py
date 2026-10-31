"""Rebuild the archived experiment and final variant, restoring final source."""
from pathlib import Path
import gzip
import hashlib
import json
import subprocess
import sys
root=Path(__file__).resolve().parent
source=Path('crates/penelope-sqlite/src/payloads.rs')
metadata=json.loads((root/'metadata.json').read_text())
final=source.read_bytes()
assert hashlib.sha256(final).hexdigest()==metadata['source_sha256'][str(source)]
nested=gzip.decompress((root/'payload-borrow-final-source-nested.rs.gz').read_bytes())
try:
    source.write_bytes(nested)
    subprocess.run([sys.executable,str(root/'build-payload-borrow.py')],check=True)
finally:
    source.write_bytes(final)
subprocess.run([sys.executable,str(root/'build-payload-borrow-final.py')],check=True)
print('Rebuilt baseline, original nested-result experiment and final borrowed variant.')
