#!/usr/bin/env python3
"""Offline burst proof of concept; requires NumPy and OpenCV.

Preserves source JPEGs. Produces best-frame, aligned-mean, and motion-gated mean
PNGs. Operates on rendered pixels, not RAW or calibrated radiance. Translation
alignment and motion gates are deliberately conservative and not portrait-ready.
ROI metrics assume the current chart/wall scene; they are not universal scores.
"""
import argparse
import json
from pathlib import Path
import cv2
import numpy as np


def gray(image):
    return cv2.cvtColor(image, cv2.COLOR_BGR2GRAY).astype(np.float32)


def sharpness(image):
    h, w = image.shape[:2]
    patch = gray(image)[int(h*.25):int(h*.7), int(w*.45):int(w*.69)]
    return float(cv2.Laplacian(cv2.GaussianBlur(patch, (0, 0), .8), cv2.CV_32F).var())


def metrics(image):
    h, w = image.shape[:2]
    wall = gray(image)[int(h*.30):int(h*.58), int(w*.76):int(w*.94)]
    residual = wall - cv2.GaussianBlur(wall, (0, 0), 2)
    row = wall.mean(axis=1).reshape(-1, 1)
    broad = row - cv2.GaussianBlur(row, (1, 0), 0, sigmaY=80)
    return {"wall_highpass_std": float(residual.std()),
            "wall_row_residual_std": float(broad.std()),
            "target_sharpness": sharpness(image)}


def align(reference, candidate):
    scale = .25
    r = cv2.resize(gray(reference), None, fx=scale, fy=scale, interpolation=cv2.INTER_AREA) / 255
    c = cv2.resize(gray(candidate), (r.shape[1], r.shape[0]), interpolation=cv2.INTER_AREA) / 255
    warp = np.eye(2, 3, dtype=np.float32)
    window = cv2.createHanningWindow((r.shape[1], r.shape[0]), cv2.CV_32F)
    shift, response = cv2.phaseCorrelate(r.copy(), c.copy(), window)
    if response > .1 and np.linalg.norm(shift) <= 3:
        warp[:, 2] = shift
    mask = np.zeros(r.shape, np.uint8)
    mask[8:-8, 8:-8] = 255
    correlation, warp = cv2.findTransformECC(r, c, warp, cv2.MOTION_TRANSLATION,
                                            (cv2.TERM_CRITERIA_COUNT | cv2.TERM_CRITERIA_EPS, 80, 1e-5), mask, 5)
    warp[:, 2] /= scale
    if not np.isfinite(correlation) or not np.isfinite(warp).all() or correlation < .95 or np.linalg.norm(warp[:, 2]) > 12:
        raise ValueError("Alignment confidence or displacement outside limits")
    size = (reference.shape[1], reference.shape[0])
    aligned = cv2.warpAffine(candidate, warp, size, flags=cv2.INTER_LINEAR | cv2.WARP_INVERSE_MAP)
    valid = cv2.warpAffine(np.ones(candidate.shape[:2], np.uint8), warp, size,
                           flags=cv2.INTER_NEAREST | cv2.WARP_INVERSE_MAP).astype(bool)
    return aligned, valid, float(correlation), warp[:, 2].tolist()


def process(group):
    paths = sorted(group.glob("frame-*.jpg"))
    if len(paths) < 2:
        raise ValueError("At least two frames required")
    frames = [cv2.imread(str(p)) for p in paths]
    if any(f is None for f in frames) or len({f.shape for f in frames}) != 1:
        raise ValueError("Invalid or mismatched frame dimensions")
    scores = [sharpness(f) for f in frames]
    best = int(np.argmax(scores))
    ref = frames[best]
    total = ref.astype(np.float32)
    gated = total.copy()
    weights = np.ones(ref.shape[:2], np.float32)
    gated_weights = weights.copy()
    smooth_ref = cv2.GaussianBlur(ref.astype(np.float32), (0, 0), 1.5)
    records = []
    for i, frame in enumerate(frames):
        if i == best:
            continue
        try:
            aligned, valid, corr, shift = align(ref, frame)
        except (cv2.error, ValueError) as error:
            records.append({"file": paths[i].name, "rejected": str(error)})
            continue
        difference = np.max(np.abs(cv2.GaussianBlur(aligned.astype(np.float32), (0, 0), 1.5) - smooth_ref), axis=2)
        motion = cv2.dilate((difference > 10).astype(np.uint8), np.ones((7, 7), np.uint8)).astype(bool)
        safe = valid & ~motion
        total += aligned.astype(np.float32) * valid[..., None]
        weights += valid
        gated += aligned.astype(np.float32) * safe[..., None]
        gated_weights += safe
        records.append({"file": paths[i].name, "correlation": corr, "shift_pixels": shift,
                        "motion_or_change_fraction": float(motion.mean())})
    outputs = {"best-frame": ref, "aligned-mean": np.rint(total / weights[..., None]).clip(0,255).astype(np.uint8),
               "motion-gated-mean": np.rint(gated / gated_weights[..., None]).clip(0,255).astype(np.uint8)}
    report = {"best_frame": paths[best].name, "frame_scores": scores, "alignments": records, "metrics": {}}
    for name, result in outputs.items():
        cv2.imwrite(str(group / (name + '.png')), result)
        report['metrics'][name] = metrics(result)
    (group / 'processing.json').write_text(json.dumps(report, indent=2))
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('experiment', type=Path)
    args = parser.parse_args()
    report = {group.name: process(group) for group in sorted(args.experiment.iterdir())
              if group.is_dir() and list(group.glob('frame-*.jpg'))}
    (args.experiment / 'processing-summary.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
