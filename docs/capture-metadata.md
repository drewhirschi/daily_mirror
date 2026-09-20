# Per-photo capture metadata

Every photograph records what took it and with what settings, so a blurry
frame can be traced to a camera, a firmware version and an exposure rather
than guessed at. This is the contract the Pi service and the ESP32 firmware
implement against; the server side is live today.

The fields are **real columns** on `photos`, not a JSON blob, so they can be
sorted and filtered. Low light is this project's known cause of blur, so
`mean_luma`, `exposure_us` and `analog_gain` are the three to reach for first.

## Sending it

Put a `capture` object on the upload-grant request body:

```
POST /api/uploads
Authorization: Bearer <per-device token>
Content-Type: application/json
```

```json
{
  "capture_id": "20260919T140311Z-1a2b3c4d",
  "content_type": "image/jpeg",
  "content_length": 812345,
  "capture": {
    "firmware_version": "0.4.1",
    "sensor": "imx519",
    "width": 4656,
    "height": 3496,
    "jpeg_quality": 90,
    "exposure_us": 19994,
    "analog_gain": 8.0,
    "digital_gain": 1.02,
    "af_state": "focused",
    "lens_position": 3.5,
    "colour_temperature_k": 2800,
    "mean_luma": 31,
    "focus_score": 412,
    "trigger": "button",
    "capture_source": "device",
    "captured_at": "2026-09-19T14:03:11Z"
  }
}
```

The same object is accepted on the guided-enrollment grant
(`POST /api/household/people/{person_id}/enrollment/uploads`), which the
mobile app uses.

**Every field is optional.** Send what the sensor can report and leave the rest
out; omitted fields are stored as NULL and hidden in the app. **Unknown fields
are ignored**, so firmware may start sending a new reading before the server
learns to store it — the two can deploy in either order.

## The fields

| Field | Type | Units and meaning |
| --- | --- | --- |
| `firmware_version` | string ≤64 | The camera's software version. Omit it and the server snapshots `devices.firmware_version` from the pairing record. |
| `sensor` | string ≤64 | Sensor part, lowercased by the server: `ov5640`, `imx519`, `imx708`. |
| `width`, `height` | integer 1–20000 | Encoded pixel dimensions, before any rotation the gallery applies later. |
| `jpeg_quality` | integer 0–100 | Normalised to 0–100 on every platform. Convert first if your encoder uses another scale. |
| `exposure_us` | integer 1–60000000 | Exposure time in **microseconds**. Same quantity as libcamera `ExposureTime`. |
| `analog_gain` | number 0–1024 | Analogue sensor gain as a **dimensionless multiplier**; 1.0 is unity, not dB and not an ISO number. |
| `digital_gain` | number 0–1024 | Digital gain, same multiplier convention. Pi only today. |
| `af_state` | `unknown` \| `searching` \| `focused` \| `failed` | Focus state at the moment of capture. |
| `lens_position` | number −10–100 | Lens position in **dioptres** (reciprocal metres), matching libcamera. Pi/IMX519 only today. |
| `colour_temperature_k` | integer 500–30000 | Estimated scene colour temperature in kelvin. Pi only today. |
| `mean_luma` | integer 0–255 | Sensor average luminance. On the OV5640 this is register `0x56A1`. The single most useful number for the blur problem. |
| `focus_score` | integer 0–1000000 | Sharpness score, when the AF firmware reports one (OV5640). Higher is sharper; the scale is sensor-specific. |
| `trigger` | `button` \| `debug` \| `schedule` \| `app` | What caused the capture. |
| `capture_source` | `device` \| `phone` \| `legacy` | What kind of client sent it. Defaulted by the server (see below) — send it only to be explicit. |
| `captured_at` | RFC 3339 UTC | When the shutter fired, for example `2026-09-19T14:03:11Z`. Offsets other than `Z` are refused. |

Values outside these ranges are refused with `400`, because a sensor reporting
a nonsense number is worth noticing rather than storing. Strings are trimmed
and capped at 64 characters.

### What the server fills in

* `capture_source` defaults to `device` for a per-device token, `legacy` for
  the shared Pi upload token, and `phone` for an enrollment upload.
* `firmware_version` falls back to the version recorded when the device paired.
* `device_id` and the camera's name come from the upload credential, not from
  this object.

## Capture IDs and `captured_at`

A capture ID must be `YYYYMMDDTHHMMSSZ-<8 lowercase hex>`, for example
`20260919T140311Z-1a2b3c4d`. The grant endpoint refuses anything else with
`400`, and refuses a timestamp before 2020 or more than a day in the future.

This is not cosmetic. The catalog derives `photos.captured_at` by slicing the
ID, so an ID in another shape stored a timestamp that sorted above every real
photograph; and because reservation upserts on the ID, a camera whose IDs
repeat could overwrite a finished photograph's row. The ESP32 firmware sent
`<device-id>-<uptime>` for a while and produced exactly that.

Send `captured_at` when the camera has a real clock: the server prefers it
over the ID-derived time. An implausible value (before 2020, or in the future)
is *dropped* rather than refused, so a camera with a bad clock still keeps its
photograph — the ID-derived time is used instead.

A completed photograph's row is never repointed: re-requesting a grant for the
same ID with the same size and device is idempotent, but a different
photograph reusing the ID is refused with `409`.

## Mapping from the Pi's libcamera sidecar

`rpicam-still --metadata-format json` writes a control block that the Pi
service already saves beside each capture as `sensor_metadata`. The grant
endpoint accepts that object verbatim alongside `capture`, so the Pi can
forward its sidecar unchanged while it is being converted:

```json
{
  "capture_id": "20260919T140311Z-1a2b3c4d",
  "content_type": "image/jpeg",
  "content_length": 812345,
  "capture": { "sensor": "imx519", "trigger": "button" },
  "sensor_metadata": {
    "ExposureTime": 19994,
    "AnalogueGain": 8.0,
    "DigitalGain": 1.02,
    "ColourTemperature": 2800,
    "LensPosition": 3.5,
    "AfState": 2
  }
}
```

| libcamera control | `capture` field | Note |
| --- | --- | --- |
| `ExposureTime` | `exposure_us` | Already microseconds. |
| `AnalogueGain` | `analog_gain` | Already a multiplier. |
| `DigitalGain` | `digital_gain` | |
| `ColourTemperature` | `colour_temperature_k` | |
| `LensPosition` | `lens_position` | Already dioptres. |
| `AfState` | `af_state` | `0→unknown`, `1→searching`, `2→focused`, `3→failed`; names are accepted too. |

Anything explicitly set in `capture` wins over the mapped value. Controls with
no mapping (`SensorTimestamp`, `FrameDuration`, `Lux`, …) are ignored.

`Lux` is deliberately not mapped: `mean_luma` is the sensor-level number this
project tunes against, and a libcamera lux estimate is not the same quantity.

## Reading it back

`GET /api/photos` returns an optional `capture` object on each photograph with
the same field names, plus `device_id` and `device_name`. The mobile photo
viewer shows a two-line block beneath the controls:

```
Captured by Kitchen Mirror · imx519 · fw 0.4.1 · 4656×3496
20 ms · gain 8× · luma 31 · af focused · lens 3.5 · q90
```

Absent fields are simply omitted, and a photograph with nothing recorded shows
no block at all.

## Adding a field

Capture metadata lives in real columns, so a new reading needs a migration.
Add it to `server/migrations/` as described in `docs/deployment.md`, add the
field to `CaptureMetadata` and `PhotoCapture` in `server/src/capture.rs` with
its range check, extend the column list in `server/src/catalog.rs`, regenerate
the typed client, and document the units here.
