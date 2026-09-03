# Focus & sharpness experiments (2026-09-02)

Empirical results from ~40 captures on the actual Pi (`rpi1.local`, Arducam
16MP IMX519) using `scripts/focus_sweep.py`. Each capture logged rpicam
metadata and was scored with variance-of-Laplacian on a center crop, then
key crops were inspected visually. Run conditions: pre-dawn living room,
**Lux ≈ 4** (very dark) — rerun in real usage light before locking anything in.

## Finding 1: autofocus works; manual dioptre calibration was a dead end

The manual lens sweep (0.0 → 3.0 dioptres) peaked at **~2.6 dioptres**
(sharpness 70.6). The production AF config (`--autofocus-mode auto
--autofocus-on-capture`, from the Pi's `.env`) landed at 2.34–2.44 on every
repeat — right at the peak. The lab default preset of **0.82 dioptres scored
3× worse** (20.4) and is visibly out of focus.

Related trap: the admin lab's focus calibration is **temporary** — only
camera orientation persists (`device/src/lab.rs:252`). Real captures use the
`.env` `DAILY_MIRROR_CAMERA_ARGS`, so lab dioptre tweaking never affected
production captures. Tweaking dioptres was never going to fix blur.

## Finding 2: light is the real bottleneck

Every auto-exposed capture metered **60 ms shutter (1/17 s) at 4.4× analogue
gain**. Two consequences:

- **Motion blur**: at 1/17 s a person swaying normally moves several mm
  during the exposure — many pixels at 16 MP portrait distance. No focus
  setting can fix this.
- **Noise smear**: high gain forces the ISP denoiser to eat fine detail
  (skin/hair texture), reading as "soft".

For crisp people you want ≤ ~10 ms (1/100 s). At Lux 4 that needs ~26× gain
(max is 16) — i.e. **the room needs more light**, not a better sensor. A ring
light / LED strip at the mirror is the single highest-impact fix.

## Finding 3: settings grid (at locked best focus)

| Setting | Metric | Visual verdict |
|---|---|---|
| baseline (defaults) | 70.9 | reference |
| `--denoise cdn_hq` | 69.5 | no visible gain |
| `--denoise off` | 82.8 | metric is noise, not detail — grainy |
| `--sharpness 0.5` | 53.5 | softer, worse |
| `--sharpness 1.5` | 83.0 | **crisper, no artifacts — safe win** |
| `--sharpness 2.0` | 99.0 | good; slight texture crunch |
| `--sharpness 3.0` | 150.1 | over-sharpened, noisy — reject |
| `--exposure sport` | 78.4 | 44 ms @ gain 6 — less motion blur, worth it |
| `--shutter 30000 --gain 8` | 85.7 | same brightness, half the motion blur |
| `--shutter 20000 --gain 12` | 112.6 | dimmer, noisier, but motion-blur-proof |

## Recommended production args

Update `DAILY_MIRROR_CAMERA_ARGS` in the Pi's
`/home/drew/daily-mirror-device/.env`:

```
--nopreview --timeout 3000 --autofocus-mode auto --autofocus-on-capture \
--width 4656 --height 3496 --encoding jpg --quality 95 \
--sharpness 1.5 --exposure sport --metadata - --metadata-format json
```

Changes vs current: `--sharpness 1.5` (visually crisper, artifact-free),
`--exposure sport` (biases toward short shutter — big deal in daylight,
mild gain cost at night), and restoring `--metadata -` so every real capture
logs `LensPosition` / `ExposureTime` / `AnalogueGain` / `AfState` to the
journal for future diagnosis (the defaults in `main.rs` include this; the
`.env` override had dropped it).

## Rerun checklist (daylight, before finalizing)

1. Put a face or newspaper where subjects actually stand, in usage lighting.
2. `scp scripts/focus_sweep.py drew@rpi1.local:~/ && ssh drew@rpi1.local 'python3 ~/focus_sweep.py --grid'`
3. Check `ExposureTime` in the output: if it's still > ~15 ms in usage
   lighting, add light at the mirror.
4. Eyeball crops in the run's `crops/` dir — the Laplacian metric inflates
   with noise and over-sharpening; trust eyes for ties.
