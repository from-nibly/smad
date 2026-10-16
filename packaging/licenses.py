#!/usr/bin/env python3
"""Bundle attribution for the locked, active workspace dependency closure (no new downloads)."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
output = Path(sys.argv[1]).resolve()
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--offline", "--format-version", "1"], cwd=root, text=True))
tree = subprocess.check_output(["cargo", "tree", "--locked", "--offline", "--workspace", "--edges", "normal,build", "--prefix", "none", "--format", "{p}"], cwd=root, text=True)
active = set(re.findall(r"^([\w-]+) v([^\s]+)", tree, re.M))
workspace = set(metadata["workspace_members"])
output.mkdir(parents=True, exist_ok=True)
inventory = []
for package in metadata["packages"]:
    if package["id"] in workspace or (package["name"], package["version"]) not in active:
        continue
    source = Path(package["manifest_path"]).parent.resolve()
    destination = output / f'{package["name"]}-{package["version"]}'
    candidates = []
    for entry in source.iterdir():
        if entry.name.lower().startswith(("license", "licence", "copying", "notice", "copyright")):
            candidates.extend(entry.rglob("*") if entry.is_dir() else [entry])
    if package.get("license_file"):
        candidates.append(source / package["license_file"])
    attribution_root = source
    if not candidates:
        attribution_root = root / "licenses/cargo-overrides" / f'{package["name"]}-{package["version"]}'
        candidates = list(attribution_root.glob("*"))
    files = []
    for path in sorted(set(candidates)):
        if not path.is_file():
            continue
        if not path.resolve().is_relative_to(attribution_root.resolve()) or path.stat().st_size > 1024 * 1024:
            raise SystemExit(f"unsafe attribution path: {path}")
        relative = path.relative_to(attribution_root)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
        files.append({"path": str(target.relative_to(output)), "sha256": hashlib.sha256(target.read_bytes()).hexdigest()})
    if not files:
        raise SystemExit(f'No attribution text found for {package["name"]} {package["version"]}; resolve before release')
    if "MPL-2.0" in (package["license"] or ""):
        # Distribute the exact, unmodified covered source rather than relying on a future URL.
        for path in source.rglob("*"):
            if path.is_symlink() and not path.resolve().is_relative_to(source):
                raise SystemExit(f"external source symlink: {path}")
        shutil.copytree(source, destination / "source", dirs_exist_ok=True)
    inventory.append({"name": package["name"], "version": package["version"], "license": package["license"], "repository": package["repository"], "files": files})
(output / "inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
print(f"Bundled license texts for {len(inventory)} dependency packages")
