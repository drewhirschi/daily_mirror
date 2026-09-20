# Burst and banding experiment — September 10, 2026

## Scope

Tested the current rpi2 IMX519 using the existing rpicam stack and a static
monitor/wall scene. No people or flash hardware were tested. Normal capture
settings are unchanged; the usual lab preview was restored after the experiments.

## Capture

`scripts/capture_burst_experiment.py` runs **on the Pi**, with the device service
stopped and restarted by the caller in a shell EXIT trap. It first autofocuses a
reference, then locks lens position, white balance, shutter and gain within each
five-frame group. It uses `rpicam-still --zsl --signal`, with completed metadata
files as capture acknowledgements. This avoids restarting the camera per photo.
A first run using stdout acknowledgements hit buffering and was abandoned.
Original JPEGs and actual per-frame metadata are retained.

Each run compares the reference shutter near 20 ms to a 16.667 ms shutter,
adjusting gain to approximately match total exposure. Full resolution is
4656×3496; `--binned` requests 2328×1748. Both retain the full field of view.
Five-frame spans measured from sensor timestamps:

| Output | Original timing | 60 Hz-compatible timing |
| --- | ---: | ---: |
| 16 MP | 2.555 s | 2.555 s |
| 4 MP | 0.633 s | 0.767 s |

These exclude autofocus, settling, file transfer and offline processing. They
are not total button-to-result times. JPEG encoding and acknowledgement incur
gaps, so this is not a demonstration of the sensor's maximum frame rate.

Raw experiments remain on rpi2 under
`/home/drew/daily-mirror-device/data/experiments/burst-20260911-02` (16 MP) and
`burst-20260911-03` (4 MP). Readiness was restored after both runs. No gallery
uploads were made. No system camera packages were installed or upgraded.

## Processing

`scripts/process_burst_experiment.py EXPERIMENT_DIRECTORY` runs offline with
NumPy and OpenCV. OpenCV 5.0.0 was installed in an isolated temporary directory
on the workstation for this run. The source frames are untouched.

1. Rank frames using a lightly smoothed sharpness metric in the chart ROI.
2. Align candidate frames using phase-correlation initialization and ECC
   translation refinement. Reject failed alignment, low correlation or large
   displacement.
3. Produce a simple aligned mean.
4. Produce a second mean that rejects pixels with large local changes, retaining
   the reference at those locations. Rejecting a whole frame is also permitted.
5. Save lossless PNG results and reports beside the original JPEGs.

This is averaging of already-rendered pixels, not calibrated radiance merging,
RAW processing, HDR, face-aware selection, or a production motion model. It
cannot restore clipped highlights. Scene-specific sharpness and wall metrics
must not be used as universal photo-quality scores.

Synthetic tests (`scripts/test_burst_processing.py`) verify known translation
recovery, actual error reduction against a clean image, and rejection of a
changed foreground patch. All three pass. Real portrait motion remains untested.

## Findings

In the 16 MP run, mean wall row-residual variation fell from 1.749 to 0.858
8-bit levels with the 60 Hz-compatible shutter. This is consistent with a
flicker contribution, not proof of the lighting source. The 4 MP run did not
show that benefit (1.728 versus 2.178). Broad illumination gradients, wall
texture, sensor timing and the changing display confound a definitive diagnosis.

For the 16 MP 60 Hz group, combining five frames reduced the wall's high-pass
variation from about 2.49 to 1.10 levels: roughly 56%. This metric includes
texture and processing artifacts, not just sensor noise. The wall visibly
looks smoother. Fine text remains readable but some edges are softer after
alignment/interpolation. The 4 MP output is faster and smoother but resolves
less fine text; its incremental stacking gain is smaller.

Detailed metadata and measured results live in `docs/evidence/burst-20260911/`.
The comparison graphic uses native-pixel crops without additional denoising.

## Next implementation choices

- First identify the lighting source and test flicker suppression in each actual
  sensor mode. The installed CLI supports `--flicker-period 8333us` for 60 Hz
  mains-related flicker; PWM or display flicker need not follow mains frequency.
- Test portrait motion and blinking. Select a good anchor frame and fall back to
  it where registration or motion checks fail. Global translation alone cannot
  align expression changes or independently moving facial features.
- Improve capture throughput by retaining consecutive buffers before encoding,
  then compare 4 MP and 16 MP at the same delivered image dimensions.
- Add exposure bracketing only after motion rejection is reliable. Mertens-style
  exposure fusion is a reasonable offline comparison, but multi-exposure ghosting
  is a separate problem from same-exposure denoising.
- For LED lighting, hold illumination steady through settling and the entire
  burst, then switch it off. Establish exposure/focus under the capture light.
  A short strobe can cause partial-frame illumination with rolling readout.
  Diffusion softens lighting but does not fix driver flicker; choose/test the
  driver and dimming mode along with the strips. No flash timing is calibrated.

References:
- [Raspberry Pi camera tuning guide](https://datasheets.raspberrypi.com/camera/raspberry-pi-camera-guide.pdf)
- [rpicam still capture implementation](https://github.com/raspberrypi/rpicam-apps/blob/main/apps/rpicam_still.cpp)
- [OpenCV HDR and exposure fusion](https://docs.opencv.org/4.13.0/d2/df0/tutorial_py_hdr.html)
