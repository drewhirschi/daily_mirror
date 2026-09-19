# mirror_config

NVS-backed device settings plus the admin page that edits them.

## API

See `include/mirror_config.h` for the authoritative contract.

| Function | Notes |
| --- | --- |
| `mirror_config_init()` | Call once, first. Runs `nvs_flash_init()` with the standard erase-and-retry on `ESP_ERR_NVS_NO_FREE_PAGES` / `ESP_ERR_NVS_NEW_VERSION_FOUND`, tolerates NVS already being initialised (`ESP_ERR_INVALID_STATE` is treated as success), then loads every setting into an in-RAM cache. |
| `mirror_config_get(key)` | Never returns NULL. Returns the NVS value if one is stored, otherwise the Kconfig default, otherwise `""`. The pointer is into the long-lived cache, so it stays valid until the next `set()` for that key. |
| `mirror_config_set(key, value)` | Writes NVS and **commits** before touching the cache. An empty value erases the key, which makes the Kconfig default visible again. |
| `mirror_config_has_wifi()` | True when an SSID is available from either source. |
| `mirror_config_erase_all()` | Clears the whole `mirror` namespace and the cache. |
| `mirror_config_register_http(server)` | Registers the three routes below on an already-started server. |

Keys are the `MIRROR_CFG_*` macros; values are UTF-8, under `MIRROR_CONFIG_VALUE_MAX` (256) bytes.

Storage lives in NVS namespace `mirror`.

### Thread safety

A static mutex guards the cache, so `get()`/`set()` are safe from any task. `get()`
returns a pointer rather than a copy — as the header requires — so a caller holding
that pointer across a concurrent `set()` of the *same* key can observe a torn value.
In practice settings are only written from the HTTP handlers, which reboot immediately
afterwards. Copy the string if you need to hold it.

## HTTP routes

`mirror_config_register_http()` registers **3 URI handlers**. Size the caller's
`httpd_config_t.max_uri_handlers` accordingly (the default is 8, so leave room).

| Route | Behaviour |
| --- | --- |
| `GET /config` | Self-contained dark HTML form (no external assets, works at phone width). Fields: Wi-Fi SSID, Wi-Fi password, server URL, upload token, device name. |
| `POST /config` | `application/x-www-form-urlencoded`. Stores, responds, then reboots ~1 s later from a short-lived task so the response flushes first. |
| `POST /config/reset` | Erases every setting, responds, reboots the same way. Fronted by a JS `confirm()` on the page. |

### Validation

Checked before anything is stored, so a rejected form never leaves a partial write:

- SSID: 1–32 bytes.
- Wi-Fi password: empty (open network) or 8–63 bytes.
- Server URL: empty, or starts with `http://` or `https://` and has something after it.
- Device name: at most 63 bytes, no control characters.

Bad input gets a `400` with a readable message and a link back to the form.
The body itself is rejected over 2 KB, and any single value over
`MIRROR_CONFIG_VALUE_MAX` is a 400 as well.

## Security notes

- **Secrets are never echoed.** The Wi-Fi password and upload token render as
  empty `type=password` inputs whose placeholder is `•••• set` or `not set`.
  A blank submission means "leave unchanged", so the form can be re-saved
  without re-typing them.
- Every stored value that *is* rendered goes through `mirror_html_escape()`
  (`& < > " '`), both as text and inside attributes.
- Values are never written to the log.
- Build-time defaults (`Kconfig`: `MIRROR_DEFAULT_WIFI_SSID`, `_WIFI_PASS`,
  `_SERVER_URL`, `_UPLOAD_TOKEN`) all default to empty. They exist so a bench
  build can carry credentials in a **git-ignored `sdkconfig`** instead of in
  source. Anything set there is baked into the firmware image in plain text.
- The routes have no authentication of their own. They are as trusted as the
  LAN the device is on; put them behind whatever the application uses.

## Layout

- `mirror_config.c` — NVS cache, HTTP handlers, deferred reboot.
- `mirror_form.c` / `mirror_form.h` — urlencoded parsing, validators and HTML
  escaping. **No ESP-IDF dependencies**, so it can be unit-tested on the host:

  ```sh
  gcc -Wall -Wextra -Werror -std=c11 -fsanitize=address,undefined \
      -Ifirmware/components/mirror_config \
      test_mirror_form.c firmware/components/mirror_config/mirror_form.c -o t && ./t
  ```
