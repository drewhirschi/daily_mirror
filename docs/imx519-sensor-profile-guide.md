# IMX519 unit: evidence and profiles to carry into portrait testing

This record describes the replacement module on **rpi2**, including its lens,
Raspberry Pi 4 camera software and image processing. It is not a specification
for every IMX519 module, nor a completed characterization of the bare sensor.
The module brand/lens model and measured target distance remain unknown.

## What we can carry forward

| Finding | Evidence | Practical consequence |
| --- | --- | --- |
| Sensor and lens control operate | Post-reboot IMX519 detection, 4656×3496 JPEGs, AK7375 binding | Hardware/software bring-up is complete for this unit |
| Stock tuning omitted autofocus | Missing `rpi.af`, explicit no-algorithm logs; adding a separate AF block enabled focused captures | Keep the custom tuning file; preserve stock files |
| Focus works at the tested positions | Focused metadata and legible small chart text over repeated captures | Retain central autofocus region; verify again on eyes/faces and at measured distances |
| Forced high-quality denoising slowed focus | Two 3-second runs were still searching; auto denoising restored three focused repeats | Use `auto` rather than forcing `cdn_hq` throughout acquisition |
| Color/exposure choices visibly matter | Saved neutral, brighter, sport and tone-curve comparisons | Start with modest saturation/sharpening and lifted shadows; avoid blanket positive EV |
| Exposure timing can affect wall bands | 16 MP row-residual proxy improved near 1/60 s; 4 MP did not show the same improvement | Test flicker per sensor mode with final lights; don't call it solved |
| Stacking can reduce visible grain | Five aligned 16 MP frames reduced wall high-pass variation about 56%, with some edge softness | Carry best-frame fallback into merging; inspect detail as well as smoothness |
| Current capture path has a speed/detail tradeoff | Five-frame sensor spans: 16 MP ~2.56 s, 4 MP ~0.63–0.77 s | Benchmark a faster acquisition path before enabling portrait bursts |

## Candidate profiles

These are reproducible starting points, not a ranking established on portraits.

| Profile | Configuration | Status and intended test |
| --- | --- | --- |
| Neutral reference | IMX519 AF tuning, stock tone curve, saturation/contrast/sharpness 1, automatic exposure/WB | Captured; retain as the control for every new lighting setup |
| Vibrant single photo | Portrait tone file (stock output gamma raised to 0.85), saturation 1.15, sharpness 1.15, contrast 1, brightness/EV 0, sport exposure, auto denoise | Saved in normal device captures; verify skin tone and facial texture |
| Detail-first burst | 4656×3496, five frames; settled focus then locked lens/WB/exposure/gain; aligned mean with rejection of local changes | Offline prototype tested on static scene; current span too long for casual portrait movement |
| Faster burst | Same method at full-field 2328×1748 | Static scene tested; faster but finer text is softer; compare at equal delivered image dimensions |
| Exposure-bracketed fusion | Multiple exposure levels, aligned with motion rejection | **Not yet tested**; intended for bright-background/dark-face scenes |

Normal and lab profiles are distinct: the device `.env` contains quoted normal
capture arguments and the tuning-file path. Lab controls are session settings.
See `evidence/imx519-rpi2/portrait-lab-settings.json` and the calibration record
for exact settings and reproduction commands. A saved `lens_position` value in
continuous mode is inactive; current distance presets are not calibrated.

## Data preserved

- Original JPEGs, reference photos and separate processed PNGs.
- Actual exposure, analogue/digital gain, white-balance gains, AF state and lens
  position where supplied; sensor timestamps for burst cadence.
- Capture commands/settings, registration results, rejected areas, and
  scene-specific wall/detail measurements.
- Local archive: `captures/characterization/2026-09-10/` (ignored by Git).
  `evidence/imx519-rpi2/archive-index.json` inventories it with hashes.
- Burst originals also remain on rpi2 under `data/experiments/`.
- Metadata/reports and scripts are in the repository working tree. The local
  archive is durable beyond `/tmp`, but is not an off-machine backup.

## What we still do not know

- Real skin-tone accuracy, eye/eyelash/hair detail, blinking and subject motion.
- The focus-distance relationship, depth of field at measured subject positions,
  and edge performance for the actual lens.
- Calibrated sensor read noise, dynamic range, color response or RAW behavior.
  JPEG grain and edge metrics mix sensor, optics, ISP, display and lighting.
- Final LED intensity, spectrum, flicker/PWM behavior, diffusion, illumination
  uniformity, and the settle time needed before a photo.
- Whether a faster burst with independently moving people beats the best single
  frame, including latency and memory costs of a complete end-to-end pipeline.

## Controlled next pass when lighting arrives

Measure distance, fix the camera and target positions, and record LED drive/
brightness/diffuser settings. Use a printed detail target and a neutral reference
under the same light as the face. Run the neutral and vibrant single profiles,
then motion-tested bursts. Change one variable at a time and repeat each case.
Compare skin and hair at the same output size, highlight retention, visible noise,
focus hit rate and time to the final image. Retain the neutral original even when
a processed version is preferred. Revisit flash timing using actual frame
acquisition, rather than assuming that a short pulse illuminates every row.

Supporting records: [bring-up and portrait tuning](imx519-rpi2-calibration.md)
and [burst experiments](burst-processing-experiment.md).
