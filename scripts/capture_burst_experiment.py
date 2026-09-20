#!/usr/bin/env python3
"""Run on the Pi with the device service stopped; preserve all experimental frames.

Uses the installed rpicam-still and Python standard library. Locks lens, white
balance, exposure, and gain within each burst. Does not change service settings.
"""
import argparse
import json
from pathlib import Path
import signal
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--count", type=int, default=5)
    parser.add_argument("--tuning", default="/home/drew/daily-mirror-device/config/imx519-portrait.json")
    parser.add_argument("--binned", action="store_true", help="Capture full-field 2328x1748 output")
    args = parser.parse_args()
    if not 2 <= args.count <= 12:
        parser.error("count must be 2..12")
    args.output.mkdir(parents=True, exist_ok=False)
    width, height = (2328, 1748) if args.binned else (4656, 3496)
    base = ["rpicam-still", "--tuning-file", args.tuning, "--nopreview",
            "--width", str(width), "--height", str(height), "--encoding", "jpg",
            "--quality", "95", "--saturation", "1.15", "--sharpness", "1.15",
            "--denoise", "auto", "--metadata-format", "json"]
    reference = base + ["--timeout", "5000", "--autofocus-mode", "continuous",
                        "--autofocus-speed", "fast", "--autofocus-window", "0.2,0.15,0.6,0.7",
                        "--exposure", "sport", "--metadata", str(args.output / "reference.json"),
                        "--output", str(args.output / "reference.jpg")]
    with (args.output / "reference.log").open("w") as log:
        subprocess.run(reference, stderr=log, check=True, timeout=30)
    ref = json.loads((args.output / "reference.json").read_text())
    if ref.get("AfState") != 2:
        raise RuntimeError("Reference autofocus did not converge; experiment stopped")
    total = ref["ExposureTime"] * ref["AnalogueGain"] * ref.get("DigitalGain", 1)
    manifest = {"reference": ref, "groups": []}
    for name, shutter in [("original-timing", ref["ExposureTime"]), ("60hz-timing", 16667)]:
        group = args.output / name
        group.mkdir()
        gain = total / shutter
        if not 1 <= gain <= 16:
            raise RuntimeError("Matched exposure would exceed allowed gain")
        command = base + ["--timeout", "0", "--zsl", "--signal",
                          "--autofocus-mode", "manual", "--lens-position", str(ref["LensPosition"]),
                          "--shutter", str(shutter), "--gain", str(gain), "--awbgains",
                          ",".join(map(str, ref["ColourGains"])),
                          "--metadata", str(group / "latest.json"), "--output", str(group / "frame-%02d.jpg")]
        records = []
        with (group / "camera.log").open("w") as log:
            process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=log, text=True)
            try:
                time.sleep(2)
                for index in range(args.count):
                    if process.poll() is not None:
                        raise RuntimeError("Camera exited early; see camera.log")
                    sent = time.monotonic()
                    process.send_signal(signal.SIGUSR1)
                    deadline = time.monotonic() + 20
                    while True:
                        try:
                            md = json.loads((group / "latest.json").read_text())
                            if not records or md["SensorTimestamp"] != records[-1]["SensorTimestamp"]:
                                break
                        except (OSError, ValueError):
                            pass
                        if time.monotonic() >= deadline or process.poll() is not None:
                            raise RuntimeError("Timed out waiting for frame metadata; see camera.log")
                        time.sleep(0.01)
                    md["request_to_saved_seconds"] = time.monotonic() - sent
                    (group / f"frame-{index:02d}.json").write_text(json.dumps(md, indent=2))
                    records.append(md)
                    print(name, index, md["SensorTimestamp"], md["ExposureTime"], flush=True)
            finally:
                if process.poll() is None:
                    process.send_signal(signal.SIGUSR2)
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.terminate()
                        process.wait(timeout=5)
        times = [md["SensorTimestamp"] for md in records]
        manifest["groups"].append({"name": name, "command": command, "frames": records,
                                    "capture_span_seconds": (times[-1] - times[0]) / 1e9})
        (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2))
    print("Saved", args.output, flush=True)


if __name__ == "__main__":
    main()
