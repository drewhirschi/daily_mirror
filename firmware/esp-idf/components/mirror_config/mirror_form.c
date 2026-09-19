/*
 * Form parsing and validation for the Daily Mirror settings page.
 * No ESP-IDF headers here on purpose: see mirror_form.h.
 */

#include "mirror_form.h"

#include <string.h>

static int hex_val(char c)
{
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

int mirror_form_decode(const char *src, size_t src_len, char *out, size_t out_size)
{
    if (out == NULL || out_size == 0) return MIRROR_FORM_ERR_ARGS;
    out[0] = '\0';
    if (src == NULL && src_len != 0) return MIRROR_FORM_ERR_ARGS;

    size_t w = 0;
    for (size_t r = 0; r < src_len; r++) {
        char c = src[r];
        if (c == '+') {
            c = ' ';
        } else if (c == '%') {
            /* Need exactly two more hex digits; "%", "%2" and "%zz" are errors. */
            /* Any failure leaves out empty rather than half-decoded. */
            if (r + 2 >= src_len) {
                /* fewer than two characters remain: "...%" or "...%2" */
                out[0] = '\0';
                return MIRROR_FORM_ERR_BAD_ESCAPE;
            }
            int hi = hex_val(src[r + 1]);
            int lo = hex_val(src[r + 2]);
            if (hi < 0 || lo < 0) {
                out[0] = '\0';
                return MIRROR_FORM_ERR_BAD_ESCAPE;
            }
            int byte = (hi << 4) | lo;
            if (byte == 0) { /* no embedded NUL in a C string */
                out[0] = '\0';
                return MIRROR_FORM_ERR_BAD_ESCAPE;
            }
            c = (char)byte;
            r += 2;
        }
        if (w + 1 >= out_size) {
            out[0] = '\0';
            return MIRROR_FORM_ERR_TOO_LONG;
        }
        out[w++] = c;
    }
    out[w] = '\0';
    return MIRROR_FORM_OK;
}

/* Decode a key (same rules) into a small scratch buffer for comparison. */
static bool key_matches(const char *src, size_t src_len, const char *key)
{
    char buf[64];
    if (mirror_form_decode(src, src_len, buf, sizeof(buf)) != MIRROR_FORM_OK) return false;
    return strcmp(buf, key) == 0;
}

int mirror_form_get(const char *body, size_t body_len, const char *key,
                    char *out, size_t out_size)
{
    if (key == NULL || out == NULL || out_size == 0) return MIRROR_FORM_ERR_ARGS;
    out[0] = '\0';
    if (body == NULL && body_len != 0) return MIRROR_FORM_ERR_ARGS;

    size_t pos = 0;
    while (pos < body_len) {
        size_t end = pos;
        while (end < body_len && body[end] != '&') end++;

        size_t eq = pos;
        while (eq < end && body[eq] != '=') eq++;

        if (key_matches(body + pos, eq - pos, key)) {
            const char *vstart = (eq < end) ? body + eq + 1 : body + end;
            size_t vlen = (eq < end) ? end - eq - 1 : 0;
            return mirror_form_decode(vstart, vlen, out, out_size);
        }
        pos = (end < body_len) ? end + 1 : end;
    }
    return MIRROR_FORM_ERR_NOT_FOUND;
}

const char *mirror_validate_ssid(const char *ssid)
{
    if (ssid == NULL) return "Wi-Fi SSID missing";
    size_t n = strlen(ssid);
    if (n < 1) return "Wi-Fi SSID must not be empty";
    if (n > 32) return "Wi-Fi SSID must be at most 32 bytes";
    return NULL;
}

const char *mirror_validate_pass(const char *pass)
{
    if (pass == NULL) return "Wi-Fi password missing";
    size_t n = strlen(pass);
    if (n == 0) return NULL; /* open network */
    if (n < 8) return "Wi-Fi password must be at least 8 characters (or empty for an open network)";
    if (n > 63) return "Wi-Fi password must be at most 63 characters";
    return NULL;
}

const char *mirror_validate_url(const char *url)
{
    if (url == NULL) return "Server URL missing";
    if (url[0] == '\0') return NULL;
    if (strncmp(url, "http://", 7) == 0 && url[7] != '\0') return NULL;
    if (strncmp(url, "https://", 8) == 0 && url[8] != '\0') return NULL;
    return "Server URL must start with http:// or https://";
}

const char *mirror_validate_device_name(const char *name)
{
    if (name == NULL) return "Device name missing";
    size_t n = strlen(name);
    if (n > 63) return "Device name must be at most 63 bytes";
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)name[i];
        if (c < 0x20 || c == 0x7f) return "Device name must not contain control characters";
    }
    return NULL;
}

size_t mirror_html_escape(const char *in, char *out, size_t out_size)
{
    if (out == NULL || out_size == 0) return 0;
    out[0] = '\0';
    if (in == NULL) return 0;

    size_t w = 0;
    for (size_t i = 0; in[i] != '\0'; i++) {
        const char *rep = NULL;
        switch (in[i]) {
        case '&':  rep = "&amp;";  break;
        case '<':  rep = "&lt;";   break;
        case '>':  rep = "&gt;";   break;
        case '"':  rep = "&quot;"; break;
        case '\'': rep = "&#39;";  break;
        default:   break;
        }
        if (rep != NULL) {
            size_t rl = strlen(rep);
            if (w + rl + 1 > out_size) break;
            memcpy(out + w, rep, rl);
            w += rl;
        } else {
            if (w + 2 > out_size) break;
            out[w++] = in[i];
        }
    }
    out[w] = '\0';
    return w;
}
