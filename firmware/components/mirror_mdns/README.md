# mirror_mdns

LAN discovery: a `mirror-xxxxxx.local` hostname and a `_dailymirror._tcp` service
whose TXT records let the app match a device on the network to a device in the
household.

## API

See `include/mirror_mdns.h` for the authoritative contract.

| Function | Notes |
| --- | --- |
| `mirror_device_id(char out[13])` | 12 lowercase hex chars of the station MAC, NUL-terminated. |
| `mirror_mdns_start(info)` | Call after the network interface has an address. Safe to call twice: the second call updates the advertisement rather than failing. |
| `mirror_mdns_set_claimed(bool)` | Updates the `claimed` TXT item in place. Returns `ESP_ERR_INVALID_STATE` before `start()`. |
| `mirror_mdns_hostname()` | `"mirror-xxxxxx"` without `.local`; valid after `start()`. |

## What is advertised

- Hostname: `mirror-<last 3 MAC bytes, hex>` → `mirror-a1b2c3.local`
- Instance name: `Daily Mirror camera`
- Service `_dailymirror._tcp` on `info->http_port` (falling back to 80 if zero), TXT:
  - `id` — the 12-hex device id
  - `board` — `info->board`, e.g. `esp32p4-imx519`
  - `fw` — `info->fw_version`
  - `claimed` — `"0"` or `"1"`
- Service `_http._tcp` on the same port, no TXT.

Verify from a Linux host with:

```sh
avahi-browse -rpt _dailymirror._tcp
```

## MAC address, and the ESP32-P4

`mirror_device_id()` and the hostname both come from the station MAC, resolved in
this order:

1. `esp_read_mac(mac, ESP_MAC_WIFI_STA)` — the normal path on the ESP32-S3.
2. `esp_wifi_get_mac(WIFI_IF_STA, mac)` — ask the driver for the address the
   interface actually uses.
3. `esp_read_mac(mac, ESP_MAC_BASE)` — the host chip's own efuse MAC.

Step 1 **cannot succeed on the ESP32-P4**: the P4 has no radio of its own, so
`SOC_WIFI_SUPPORTED` is undefined and IDF's `generate_mac()` has no
`ESP_MAC_WIFI_STA` case, returning `ESP_ERR_NOT_SUPPORTED`. Step 2 is the one
that yields the real radio address there, via `esp_wifi_remote`/`esp_hosted`,
and it only answers once `esp_wifi_init()` has run — which is true by the time
`mirror_mdns_start()` is legitimately called.

Results from steps 1 and 2 are cached, so the id cannot change under a caller.
The step 3 fallback is deliberately *not* cached: calling `mirror_device_id()`
before Wi-Fi is up on a P4 would otherwise freeze the host chip's efuse MAC as
the device id for the whole boot, instead of upgrading to the radio's address.
If you need the id to match what the server sees, read it after Wi-Fi starts.

## Dependencies

The managed component `espressif/mdns` (`idf_component.yml`). Only public
`esp_wifi` / `esp_mac` APIs are used, so the same source builds for both the
ESP32-S3 and the ESP32-P4; there is no direct dependency on `esp_hosted`.
