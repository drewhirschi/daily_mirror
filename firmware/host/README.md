# Daily Mirror host firmware

The device firmware, running on Linux. Same `daily-mirror-core` state machine,
same driver loop, same upload protocol as the board — only the six adapters
differ. This is where the pairing flow is developed and where CI proves it
still works.

```sh
cargo run -- --store /tmp/mirror --fixture ../../feed.jpg --port 8088
```

Type at it:

| Key | Effect |
| --- | --- |
| `p` / `r` | press and release the button |
| `c` | a short click — capture, or confirm a pairing |
| `hold 5000` | enter pairing |
| `hold 20000` then `c` `c` `c` | full reset |
| `q` | quit |

The ring prints one timestamped line per change. Send credentials the way the
app would:

```sh
curl -X POST http://127.0.0.1:8088/provision \
  -H 'content-type: application/json' \
  -d '{"ssid":"home","psk":"secret","server_url":"http://127.0.0.1:3000","claim_token":"ct_..."}'
```

Get a claim token from a signed-in session with
`POST /api/devices/claim-tokens`. `GET /` on the provisioning port reports the
device id, and `GET /status` reports what the device last said about pairing.

## Scripts

`--script` replays timestamped events against a virtual clock, so a five-minute
pairing timeout costs nothing:

```sh
cargo run -- --script scripts/happy-path.txt --store /tmp/mirror-script
```

See `scripts/` for the format. The integration tests in `tests/pairing.rs` use
the same parser and driver against a mock server built on `std::net`.

## Adapters

| Trait | Here | On the P4 |
| --- | --- | --- |
| `Clock` | `std::time::Instant`, or a virtual clock | `esp_timer_get_time` |
| `Button` | stdin | GPIO 4, active low |
| `Ring` | terminal | WS2812 over RMT |
| `Camera` | a fixture JPEG | IMX519 through `esp_video` |
| `Store` | a directory | NVS plus an SD card |
| `Net` | `reqwest` + a local HTTP provisioning link | `esp_wifi_remote` + SoftAP provisioning |
