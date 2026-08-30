# Image quality roadmap

Daily Mirror should optimize for photographs that look good over years, not
merely for successful uploads. The current prototype deliberately captures and
uploads one high-quality IMX519 JPEG without server-side processing. Preserve
that simple, reliable path while the following work is evaluated.

## Hardware baseline

The Arducam module uses Sony's 16 MP IMX519: 4656×3496 active pixels, a
1/2.53-inch optical format, 1.22 µm pixels, rolling shutter, and an autofocus
f/1.75 stock lens. Those small pixels can produce excellent detail in strong,
soft light, but require more gain—and therefore show more noise—in dim parts of
the scene. A bright window on the subject with a darker room behind them is a
useful stress case for metering, exposure, and dynamic range.

An iPhone comparison is primarily a processing comparison, not a megapixel
comparison. Modern iPhones add optical stabilization plus multi-frame systems
such as the Photonic Engine, Deep Fusion, Smart HDR, and Night mode. The Pi ISP
does provide automatic exposure/gain, white balance, lens shading, colour
correction, and denoising, but one default JPEG should not be expected to match
Apple's multi-frame pipeline in difficult handheld light. A fixed camera,
repeatable lighting, careful tuning, and later server-side fusion can narrow the
gap for this specific portrait use case.

Official references:

- [Arducam IMX519 specifications](https://docs.arducam.com/Raspberry-Pi-Camera/Native-camera/16MP-IMX519/)
- [Raspberry Pi camera software and image controls](https://www.raspberrypi.com/documentation/computers/camera_software.html)
- [Apple computational and RAW capture overview](https://developer.apple.com/documentation/avfoundation/capturing-photos-in-raw-and-apple-proraw-formats)

## Establish a single-frame baseline first

- Build repeatable test scenes for skin tones, mixed indoor light, highlights,
  shadows, fine detail, and motion.
- Tune exposure, analogue gain, white balance, autofocus, denoising,
  sharpening, contrast, and JPEG quality.
- Record the camera settings and relevant sensor metadata with every capture so
  results can be compared later.
- Treat lighting, camera placement, and lens cleanliness as part of the imaging
  system; processing cannot fully recover a poor source image.
- Keep the original camera output immutable. Processed versions should be
  derived assets that can be regenerated as the pipeline improves.

## Explore a bracketed burst

A future capture may produce a short group of frames rather than one file. The
device could vary exposure time, gain, or other selected settings while keeping
focus and white balance stable. The server can then process the group without
making the button-to-camera device substantially more complicated.

Candidate server-side experiments:

- Select the sharpest or least-blurred frame.
- Fuse different exposures for highlight and shadow detail.
- Align frames before merging and reject frames with subject motion.
- Apply conservative denoising, tone mapping, color correction, and sharpening.
- Produce several reversible variants and compare them against the original.
- Generate small gallery thumbnails as derived assets so browsing never requires
  decoding every full-resolution original.

## Constraints and questions

- Full-resolution IMX519 stills are not instantaneous. Measure the sustainable
  burst rate before choosing a frame count.
- A person will move between frames, producing ghosting in naive HDR merges.
- Exposure brackets should not refocus or visibly change white balance between
  frames unless that variation is the experiment.
- Bursts increase memory, LAN transfer time, temporary disk use, and server
  processing cost. The physical-device feedback must still remain responsive.
- Decide whether a lower-resolution fast burst produces a better final image
  than a smaller number of full-resolution frames.
- Define an objective review process: side-by-side crops, skin tone, sharpness,
  noise, motion artifacts, and subjective preference.

## Possible capture-group contract

Keep the current single-JPEG API operational. A later API can accept a capture
group containing a stable group ID, ordered original frames, per-frame camera
settings, device time, and sensor metadata. Processing should run
asynchronously on the server and publish a chosen display image while retaining
the originals for future reprocessing.

## Suggested sequence

1. Tune and document the best dependable single-frame configuration.
2. Store capture metadata and immutable originals.
3. Measure two-, three-, and five-frame burst timing at useful resolutions.
4. Build an offline comparison harness on the server.
5. Test best-frame selection before attempting multi-frame fusion.
6. Add exposure fusion only if it consistently beats the best single frame.
