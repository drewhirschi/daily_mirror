/*
 * Pure-C helpers for the settings form: application/x-www-form-urlencoded
 * parsing and field validation. Deliberately free of ESP-IDF dependencies so
 * the tricky parts can be unit-tested on the host with plain gcc.
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Result codes shared by the decode/lookup helpers. */
typedef enum {
    MIRROR_FORM_OK = 0,
    MIRROR_FORM_ERR_BAD_ESCAPE = -1, /* "%" not followed by two hex digits */
    MIRROR_FORM_ERR_TOO_LONG   = -2, /* decoded value does not fit out_size */
    MIRROR_FORM_ERR_NOT_FOUND  = -3, /* key absent from the body */
    MIRROR_FORM_ERR_ARGS       = -4, /* NULL argument or zero-sized buffer */
} mirror_form_err_t;

/**
 * Percent-decode one form value in place into @p out.
 *
 * "+" becomes a space, "%XX" becomes the byte XX (hex, either case).
 * A trailing "%" or "%2" is rejected as MIRROR_FORM_ERR_BAD_ESCAPE rather than
 * silently truncated. Embedded NULs (%00) are rejected as well, since every
 * stored value is a C string. @p out is always NUL-terminated on success.
 *
 * @param src      start of the raw (still encoded) value
 * @param src_len  its length in bytes, not NUL-terminated
 * @param out      destination buffer
 * @param out_size its size, including room for the terminator
 */
int mirror_form_decode(const char *src, size_t src_len, char *out, size_t out_size);

/**
 * Find @p key in an urlencoded body and write its decoded value to @p out.
 *
 * Keys are compared after decoding, so "device%5Fname" matches "device_name".
 * The FIRST occurrence wins; later duplicates are ignored (a browser only
 * sends one value per field, and picking the first is the predictable choice).
 * A key present with an empty value returns MIRROR_FORM_OK and out[0] == '\0';
 * a key that is absent returns MIRROR_FORM_ERR_NOT_FOUND.
 */
int mirror_form_get(const char *body, size_t body_len, const char *key,
                    char *out, size_t out_size);

/* Validators. Each returns NULL when the value is acceptable, or a short
 * human-readable message suitable for a 400 response body. */

/** SSID: 1-32 bytes. */
const char *mirror_validate_ssid(const char *ssid);

/** WPA2 passphrase: empty (open network) or 8-63 bytes. */
const char *mirror_validate_pass(const char *pass);

/** Server URL: empty, or starting with "http://" or "https://". */
const char *mirror_validate_url(const char *url);

/** Device name: 0-63 bytes, no control characters. */
const char *mirror_validate_device_name(const char *name);

/**
 * Length of the HTML escaping of @p in (excluding the terminator), and the
 * escaping itself. Escapes & < > " ' so stored values can be rendered both as
 * text and inside double-quoted attributes. Writes at most out_size-1 bytes
 * and always terminates; returns the number of bytes written.
 */
size_t mirror_html_escape(const char *in, char *out, size_t out_size);

#ifdef __cplusplus
}
#endif
