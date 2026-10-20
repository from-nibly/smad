#!/usr/bin/env python3
"""Verify pinned syntax assets locally, without fetching or executing upstream code."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
manifest = json.loads((ROOT / "assets/syntaxes/upstream.json").read_text())
for entry in manifest:
    asset = ROOT / "assets/syntaxes" / entry["file"]
    assert asset.parent == ROOT / "assets/syntaxes"
    assert hashlib.sha256(asset.read_bytes()).hexdigest() == entry["sha256"], entry["name"]
    assert len(entry["revision"]) == 40 and all(c in "0123456789abcdef" for c in entry["revision"])
    assert (ROOT / entry["license_file"]).is_file()
print(f"Verified {len(manifest)} pinned upstream syntax definitions and notices")
