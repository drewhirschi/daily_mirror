#!/usr/bin/env python3
"""Measure built Vercel function trees and an equivalent monolith executable."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--baseline", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
build = root / "server/.vercel/output"
manifest = json.loads((build / "bundle-manifest.json").read_text())
baseline = args.baseline.stat().st_size
source_paths = subprocess.check_output(
    ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z",
     "server", "processor", "crates/vision-contract", "vendor/vercel_runtime"], cwd=root,
).decode().split("\0")
source_hashes = {
    name: hashlib.sha256((root / name).read_bytes()).hexdigest()
    for name in sorted(set(source_paths)) if name and (root / name).is_file()
    and (Path(name).suffix in {".rs", ".toml", ".lock", ".tsx", ".ts"}
         or Path(name).name == "package-lock.json")
}
report = {
    "measurement": "local Linux Vercel artifact file lengths; provider layers unavailable",
    "source_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
    "working_tree_diff_sha256": hashlib.sha256(subprocess.check_output(["git", "diff"], cwd=root)).hexdigest(),
    "source_file_sha256": source_hashes,
    "framework_revision": "9fa4141d145f5afba869a42857e601c08a74e5fc",
    "toolchain": subprocess.check_output(["rustc", "--version"], text=True).strip(),
    "target": "x86_64-unknown-linux-gnu.2.26",
    "profile": "release (Cargo defaults)",
    "baseline_executable_bytes": baseline,
    "baseline_executable_sha256": hashlib.sha256(args.baseline.read_bytes()).hexdigest(),
    "baseline_package_bytes": None,
    "baseline_features": ["image-processing", "face-inference"],
    "functions": {},
}
for directory in sorted((build / "functions").rglob("*.func")):
    name = directory.stem
    totals = dict(executable=0, native_libraries=0, models_assets=0, other=0)
    files = []
    for path in sorted(directory.rglob("*")):
        if path.is_symlink():
            raise SystemExit(f"Unresolved symlink in function tree: {path}")
        if not path.is_file():
            continue
        relative = str(path.relative_to(directory))
        size = path.stat().st_size
        if relative == "executable":
            category = "executable"
        elif ".so" in path.name:
            category = "native_libraries"
        elif any(relative == asset or relative.startswith(asset.rstrip("/") + "/")
                 for asset in manifest["bundles"][name]["assets"]):
            category = "models_assets"
        else:
            category = "other"
        totals[category] += size
        files.append({"path": relative, "bytes": size, "category": category,
                      "sha256": hashlib.sha256(path.read_bytes()).hexdigest()})
    total = sum(totals.values())
    report["functions"][name] = {
        "features": manifest["bundles"][name]["features"],
        "bytes": totals, "total_bytes": total, "total_mib": total / 1048576,
        "headroom_before_provider_layers_bytes": 250_000_000 - total,
        "files": files,
    }
report["combined_function_bytes"] = sum(f["total_bytes"] for f in report["functions"].values())
default = report["functions"]["default"]["bytes"]["executable"]
report["default_executable_reduction_bytes"] = baseline - default
report["default_executable_reduction_percent"] = 100 * (baseline - default) / baseline
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(report, indent=2) + "\n")
print("| Function | Executable bytes | Package bytes | Package MiB |")
print("| --- | ---: | ---: | ---: |")
print(f"| Unsplit baseline | {baseline:,} | Not packaged | — |")
for name, entry in report["functions"].items():
    print(f"| {name} | {entry['bytes']['executable']:,} | {entry['total_bytes']:,} | {entry['total_mib']:.2f} |")
print(f"\nDefault executable reduction: {report['default_executable_reduction_bytes']:,} bytes "
      f"({report['default_executable_reduction_percent']:.2f}%).")
print("Package totals include local metadata; provider-layer headroom is provisional.")
