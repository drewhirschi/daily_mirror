#!/usr/bin/env python3
"""Focus/sharpness experiment harness for the daily-mirror IMX519 camera.

Runs ON THE PI (needs python3-numpy and python3-pil, both present on rpi1):

    scp scripts/focus_sweep.py drew@rpi1.local:~/
    ssh drew@rpi1.local 'python3 ~/focus_sweep.py'

For a meaningful run, put a detailed target (a face, or a newspaper page)
where subjects actually stand, in the light conditions the mirror is used in.

Each capture records rpicam metadata (LensPosition, ExposureTime,
AnalogueGain, Lux, FocusFoM) and a sharpness score: variance of the
Laplacian over a center crop. Higher is sharper, but the score inflates
with sensor noise — always eyeball the crops in <out>/crops/ before
trusting small differences, especially across different gain settings.

Results: ~/focus_sweep/<timestamp>/ with results.json and crops/.
"""
import argparse
import json
import os
import subprocess
import time

import numpy as np
from PIL import Image

CAM = os.environ.get("DAILY_MIRROR_CAMERA_COMMAND", "/usr/bin/rpicam-still")
BASE = ["--nopreview", "--width", "4656", "--height", "3496",
        "--encoding", "jpg", "--quality", "95"]


def laplacian_var(gray):
    g = gray.astype(np.float64)
    lap = (-4 * g[1:-1, 1:-1] + g[:-2, 1:-1] + g[2:, 1:-1]
           + g[1:-1, :-2] + g[1:-1, 2:])
    return float(lap.var())


def center_score(path):
    a = np.asarray(Image.open(path).convert("L"))
    h, w = a.shape
    return laplacian_var(a[h // 2 - 600:h // 2 + 600, w // 2 - 600:w // 2 + 600])


def save_crop(path, crops_dir):
    im = Image.open(path)
    w, h = im.size
    im.crop((w // 2 - 450, h // 2 - 450, w // 2 + 450, h // 2 + 450)).save(
        os.path.join(crops_dir, "crop-" + os.path.basename(path)), quality=92)


def capture(out_dir, name, extra, timeout_ms):
    jpg = os.path.join(out_dir, name + ".jpg")
    meta = os.path.join(out_dir, name + ".json")
    cmd = [CAM] + BASE + ["--timeout", str(timeout_ms), "--metadata", meta,
                          "--metadata-format", "json", "-o", jpg] + extra
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=90)
    if r.returncode != 0 or not os.path.exists(jpg):
        return {"name": name, "args": extra, "error": r.stderr.strip()[-400:]}
    with open(meta) as f:
        md = json.load(f)
    return {
        "name": name, "args": extra,
        "sharp_center": round(center_score(jpg), 1),
        "LensPosition": md.get("LensPosition"),
        "AfState": md.get("AfState"),
        "FocusFoM": md.get("FocusFoM"),
        "ExposureTime_us": md.get("ExposureTime"),
        "AnalogueGain": md.get("AnalogueGain"),
        "Lux": md.get("Lux"),
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--start", type=float, default=0.0, help="sweep start (dioptres)")
    p.add_argument("--stop", type=float, default=3.0, help="sweep stop (dioptres)")
    p.add_argument("--step", type=float, default=0.25, help="sweep step (dioptres)")
    p.add_argument("--af-repeats", type=int, default=3,
                   help="repeats of the production AF config (auto + on-capture)")
    p.add_argument("--skip-sweep", action="store_true", help="AF/exposure tests only")
    p.add_argument("--grid", action="store_true",
                   help="also run the settings grid (denoise/sharpness/shutter) "
                        "at the sharpest lens position found")
    args = p.parse_args()

    out = os.path.expanduser("~/focus_sweep/run-%s" % time.strftime("%Y%m%d-%H%M%S"))
    crops = os.path.join(out, "crops")
    os.makedirs(crops, exist_ok=True)
    results = []

    def run(name, extra, timeout_ms):
        r = capture(out, name, extra, timeout_ms)
        results.append(r)
        print(json.dumps(r), flush=True)
        if "error" not in r:
            save_crop(os.path.join(out, name + ".jpg"), crops)

    # Production AF config (matches the Pi's DAILY_MIRROR_CAMERA_ARGS)
    for i in range(args.af_repeats):
        run("af-auto-%d" % i,
            ["--autofocus-mode", "auto", "--autofocus-on-capture"], 3000)

    # Same but biased toward short shutter — the anti-motion-blur variant
    run("af-auto-sport",
        ["--autofocus-mode", "auto", "--autofocus-on-capture",
         "--exposure", "sport"], 3000)

    if not args.skip_sweep:
        lp = args.start
        while lp <= args.stop + 1e-9:
            run("manual-%.2f" % lp,
                ["--autofocus-mode", "manual", "--lens-position", "%.2f" % lp],
                1500)
            lp += args.step

    ok = [r for r in results if "error" not in r]
    best_lp = 2.55
    if ok:
        best = max(ok, key=lambda r: r["sharp_center"])
        if best["LensPosition"]:
            best_lp = best["LensPosition"]
        print("BEST: %s  sharp=%.1f  LensPosition=%s" %
              (best["name"], best["sharp_center"], best["LensPosition"]))

    if args.grid:
        lock = ["--autofocus-mode", "manual", "--lens-position", "%.2f" % best_lp]
        for name, extra in [
            ("grid-baseline", []),
            ("grid-dn-cdn_hq", ["--denoise", "cdn_hq"]),
            ("grid-dn-off", ["--denoise", "off"]),
            ("grid-sh-1.5", ["--sharpness", "1.5"]),
            ("grid-sh-2.0", ["--sharpness", "2.0"]),
            ("grid-sport", ["--exposure", "sport"]),
            ("grid-shut-30ms-g8", ["--shutter", "30000", "--gain", "8"]),
            ("grid-shut-20ms-g12", ["--shutter", "20000", "--gain", "12"]),
        ]:
            run(name, lock + extra, 1500)

    with open(os.path.join(out, "results.json"), "w") as f:
        json.dump(results, f, indent=2)
    print("OUT_DIR=" + out)


if __name__ == "__main__":
    main()
