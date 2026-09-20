# What is next after the integration merge

Written 2026-09-19 for the PR that lands household onboarding, device pairing
and the ESP32 camera firmware together. This is the short, ordered list. The
long version for the device side is [firmware-roadmap.md](firmware-roadmap.md);
the pairing protocol is [device-pairing-plan.md](device-pairing-plan.md).

## Right after merging

1. **Watch the deploy.** Merging to main deploys the server. The photo catalog
   gains three columns on first start (`device_id`, `source`,
   `enrollment_person_id`); all are additive with defaults, so existing photos
   are untouched. Check the gallery loads and one Pi upload still lands.
2. **Close PR 13.** Its commits are included here.
3. **Rebuild the iOS app.** The app config gained the camera permission
   (enrollment) and the Espressif provisioning plugin (pairing), so it needs a
   fresh native build, not just a JS reload.

## Twenty minutes at the bench

These are the things nobody has physically done yet.

1. Wire the buttons and the P4's RGB LED from the drawings in
   [hardware/wiring/](../hardware/wiring/) and press them. The press flow has
   only been triggered over HTTP.
2. Put a device upload token into each board's settings page and take one
   photo. The upload code has never run end to end on the ESP32s.
3. Forget Wi-Fi on one board, join its `mirror-xxxxxx` network from a phone
   and open `http://10.10.0.1/config`. Only the serial log has confirmed this
   path, because the dev machine has no Wi-Fi radio.
4. Reseat the P4's camera ribbon. It dropped off the bus twice.
5. Shoot both boards at the real mirror distance in daylight and compare.

## The next PR: make the devices pair with the app

The app's "Add a mirror" flow and the server's claim endpoints are in this
merge, but the C firmware does not speak their protocol yet. In order:

1. Espressif's provisioning manager on the device when it has no Wi-Fi:
   service name `mirror-<hex>`, security 2, BLE transport (what the app uses
   today).
2. The custom `daily-mirror` provisioning endpoint that accepts the claim
   payload, per `mobile/src/pairing/contract.ts`.
3. Redeem the claim with the server, keep the per-device token in NVS, upload
   with it, flip the mDNS `claimed` record.
4. Button gestures: long-press to pair, confirm press, 20 s reset.
5. Then BLE as a second transport on the S3.

Test each step against `firmware/host`, which already runs the same state
machine against the real server.

## After that

- A durable upload queue in flash. Today a photo lives in RAM until the next
  press and is lost if the upload fails.
- The OTA client. The two-slot partition layout is already in place, so this
  is the last change that needs a USB cable.
- P4 image tuning against the real light panel.
- Decide the product board, and whether `firmware/esp32p4` (the Rust skeleton
  from the pairing work, superseded by the C firmware) is deleted.

## Parked on purpose

Licensing of the borrowed pieces (the GPL IMX519 register tables, the OV5640
autofocus microcode) and the project's own license. Fine for a personal
project; revisit before selling hardware. Details are in
`firmware/esp-idf/NOTICE`.
