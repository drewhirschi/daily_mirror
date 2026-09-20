#include <dirent.h>
#include <stdarg.h>
#include <stddef.h>
#include <errno.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#include "mirror_spool.h"
#include "mirror_board.h"
#include "mirror_config.h"
#include "mirror_mdns.h"
#include "mirror_net.h"
#include "mirror_pair.h"
#include "mirror_press.h"
#include "mirror_ring.h"
#include "mirror_upload.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#include "esp_log.h"
#include "esp_littlefs.h"
#include "esp_random.h"
#include "esp_timer.h"
#include "esp_event.h"
#include "esp_netif.h"
#include "esp_app_desc.h"
#include "sdkconfig.h"

static const char *TAG = "mirror_spool";

#define SPOOL_LABEL   "spool"
#define SPOOL_BASE    "/spool"
#define SPOOL_Q       SPOOL_BASE "/q"
#define SPOOL_R       SPOOL_BASE "/r"
#define SPOOL_TMP     SPOOL_BASE "/tmp.dm"

#define REJECTED_MAX  2

/* Leave a little slack: LittleFS needs blocks for its own metadata, and a
 * filesystem driven to literally zero free bytes fails in less tidy ways than
 * one that is asked to stop a bit early. */
#define RESERVE_BYTES (64 * 1024)

#define BACKOFF_MIN_MS (30 * 1000)
#define BACKOFF_MAX_MS (15 * 60 * 1000)

#define ENTRY_MAGIC 0x314d4453u   /* "SDM1" */

/*
 * One queued capture: this header, then `meta_len` bytes of JSON, then
 * `jpeg_len` bytes of JPEG. Written to SPOOL_TMP and renamed into the queue,
 * so a file that exists under q/ is a file that was written all the way
 * through - the power-loss case leaves a stale tmp.dm, which start-up deletes.
 *
 * `jpeg_offset` is where the JPEG starts, stored rather than computed. It is
 * redundant with sizeof(entry_hdr_t) + meta_len and it exists because that
 * inference was wrong once already, in the worst possible way: during
 * bring-up this struct lost two fields (44 packed bytes to 32) without the
 * version being bumped, so the reader seeked 12 bytes short of the JPEG and
 * uploaded twelve bytes of trailing metadata followed by a JPEG missing its
 * end-of-image marker. Every byte count checked out - the object was exactly
 * content_length - and the only symptom was the server failing to decode it.
 * A stored offset cannot drift from the writer's layout; a computed one can.
 *
 * Version 1 entries have no such field. See legacy_header_bytes().
 */
#define ENTRY_VERSION 2

typedef struct __attribute__((packed)) {
    uint32_t magic;
    uint32_t version;
    uint32_t seq;
    uint32_t meta_len;
    uint32_t jpeg_len;
    int64_t  captured_at;     /* UTC seconds, or 0 if the clock was unset */
    uint32_t flags;
    uint32_t jpeg_offset;     /* v2+: bytes from the start of the file */
} entry_hdr_t;

/*
 * Two different version-1 layouts were written to flash during bring-up, and
 * they share a version number, so the only honest way to tell them apart is
 * arithmetic: the header size is whichever candidate makes the file's own
 * length add up. meta_len and jpeg_len sit at the same offsets in both, so
 * they are trustworthy either way.
 *
 *   44  the original: ... captured_at, uptime_us, boot_id, flags
 *   32  after the clock handling was simplified: ... captured_at, flags
 *
 * Returns 0 if neither fits, which means the entry is not recoverable.
 */
static uint32_t legacy_header_bytes(uint32_t meta_len, uint32_t jpeg_len, off_t file_size)
{
    static const uint32_t candidates[] = { 44, 32 };
    for (size_t i = 0; i < sizeof(candidates) / sizeof(candidates[0]); i++) {
        if ((off_t)candidates[i] + (off_t)meta_len + (off_t)jpeg_len == file_size) {
            return candidates[i];
        }
    }
    return 0;
}

static bool     s_mounted;
static uint32_t s_next_seq = 1;
static uint32_t s_count;
static uint64_t s_bytes;

static uint32_t s_uploads_ok;
static uint32_t s_uploads_failed;
static uint32_t s_drops;
static uint32_t s_rejected;
static int      s_last_http_status;
static char     s_last_error[120] = "";
static bool     s_token_bad;
static volatile bool s_uploading;
static int64_t  s_next_retry_us;      /* esp_timer value, 0 = now */
static uint32_t s_backoff_ms;
static char     s_last_meta[384];
static uint32_t s_failed_seq;         /* the entry the red triple was last shown for */

static SemaphoreHandle_t s_lock;      /* guards the filesystem and the counters */
static TaskHandle_t s_task;

const char *mirror_trigger_name(mirror_trigger_t t)
{
    return t == MIRROR_TRIGGER_DEBUG ? "debug" : "button";
}

static void lock(void)   { if (s_lock) xSemaphoreTake(s_lock, portMAX_DELAY); }
static void unlock(void) { if (s_lock) xSemaphoreGive(s_lock); }

static void path_for(char *out, size_t cap, const char *dir, uint32_t seq)
{
    snprintf(out, cap, "%s/%010" PRIu32 ".dm", dir, seq);
}

/* ---- the queue on disk ------------------------------------------------- */

/*
 * Walk q/ and answer three questions at once: how many entries, how many
 * bytes, and what the lowest and highest sequence numbers are. Called at
 * start-up and after anything that could have gone wrong mid-way; everything
 * else keeps the counters up to date incrementally.
 */
static void rescan(uint32_t *oldest, uint32_t *newest)
{
    uint32_t lo = UINT32_MAX, hi = 0, n = 0;
    uint64_t bytes = 0;

    DIR *d = opendir(SPOOL_Q);
    if (d) {
        struct dirent *e;
        while ((e = readdir(d)) != NULL) {
            uint32_t seq = (uint32_t)strtoul(e->d_name, NULL, 10);
            /* Big enough for any name LittleFS can return, so the compiler
             * does not have to guess and -Werror=format-truncation does not
             * have to be argued with. */
            char p[sizeof(SPOOL_Q) + 260];
            snprintf(p, sizeof(p), "%s/%s", SPOOL_Q, e->d_name);
            struct stat st;
            if (stat(p, &st) != 0 || !S_ISREG(st.st_mode)) {
                continue;
            }
            /* A file too short to hold a header is a casualty of something;
             * it cannot be uploaded and it cannot be read, so it goes. */
            if ((size_t)st.st_size <= sizeof(entry_hdr_t)) {
                ESP_LOGW(TAG, "discarding truncated spool entry %s (%ld bytes)",
                         e->d_name, (long)st.st_size);
                unlink(p);
                continue;
            }
            n++;
            bytes += (uint64_t)st.st_size;
            if (seq < lo) lo = seq;
            if (seq > hi) hi = seq;
        }
        closedir(d);
    }

    s_count = n;
    s_bytes = bytes;
    if (oldest) *oldest = n ? lo : 0;
    if (newest) *newest = n ? hi : 0;
}

/*
 * Read an entry's header and leave `hdr->jpeg_offset` correct whatever version
 * wrote it. On success with `out_f`, the file is left positioned at the first
 * JPEG byte - the caller never seeks for itself.
 */
static bool read_header(const char *path, entry_hdr_t *hdr, FILE **out_f)
{
    struct stat st;
    if (stat(path, &st) != 0) {
        return false;
    }
    FILE *f = fopen(path, "rb");
    if (!f) {
        return false;
    }
    memset(hdr, 0, sizeof(*hdr));
    /* Only the fields up to `flags` are common to every version; read those
     * first so a v1 entry is not asked for four bytes it does not have. */
    const size_t common = offsetof(entry_hdr_t, jpeg_offset);
    if (fread(hdr, 1, common, f) != common || hdr->magic != ENTRY_MAGIC) {
        fclose(f);
        return false;
    }
    if (hdr->version >= 2) {
        if (fread(&hdr->jpeg_offset, 1, sizeof(hdr->jpeg_offset), f)
                != sizeof(hdr->jpeg_offset)) {
            fclose(f);
            return false;
        }
    } else {
        uint32_t hdr_bytes = legacy_header_bytes(hdr->meta_len, hdr->jpeg_len, st.st_size);
        if (!hdr_bytes) {
            ESP_LOGW(TAG, "%s: no version-1 layout fits (%ld bytes, meta %" PRIu32
                          ", jpeg %" PRIu32 ")",
                     path, (long)st.st_size, hdr->meta_len, hdr->jpeg_len);
            fclose(f);
            return false;
        }
        hdr->jpeg_offset = hdr_bytes + hdr->meta_len;
    }
    if ((off_t)hdr->jpeg_offset + (off_t)hdr->jpeg_len != st.st_size) {
        ESP_LOGW(TAG, "%s: sizes do not add up (offset %" PRIu32 " + jpeg %" PRIu32
                      " != %ld)", path, hdr->jpeg_offset, hdr->jpeg_len, (long)st.st_size);
        fclose(f);
        return false;
    }
    if (out_f) {
        if (fseek(f, (long)hdr->jpeg_offset, SEEK_SET) != 0) {
            fclose(f);
            return false;
        }
        *out_f = f;
    } else {
        fclose(f);
    }
    return true;
}

/*
 * The JPEG in this entry starts with FF D8 and ends with FF D9.
 *
 * Four bytes of reading that would have caught the offset bug at the device
 * instead of at the server: a truncated or misaligned image is not something
 * to keep retrying for fifteen minutes at a time, and it is not something to
 * hand to the catalog either.
 */
static bool jpeg_intact(FILE *f, const entry_hdr_t *hdr)
{
    unsigned char soi[2], eoi[2];
    long here = ftell(f);
    if (hdr->jpeg_len < 4) {
        return false;
    }
    if (fseek(f, (long)hdr->jpeg_offset, SEEK_SET) != 0
        || fread(soi, 1, 2, f) != 2
        || fseek(f, (long)(hdr->jpeg_offset + hdr->jpeg_len - 2), SEEK_SET) != 0
        || fread(eoi, 1, 2, f) != 2) {
        return false;
    }
    if (fseek(f, here, SEEK_SET) != 0) {
        return false;
    }
    return soi[0] == 0xFF && soi[1] == 0xD8 && eoi[0] == 0xFF && eoi[1] == 0xD9;
}

/* The lowest sequence number present, or 0 if the queue is empty. */
static uint32_t oldest_seq(void)
{
    uint32_t lo = 0;
    rescan(&lo, NULL);
    return lo;
}

static void delete_seq(const char *dir, uint32_t seq)
{
    char p[64];
    path_for(p, sizeof(p), dir, seq);
    struct stat st;
    if (stat(p, &st) == 0) {
        unlink(p);
    }
}

static size_t fs_free_bytes(void)
{
    size_t total = 0, used = 0;
    if (esp_littlefs_info(SPOOL_LABEL, &total, &used) != ESP_OK) {
        return 0;
    }
    return used < total ? total - used : 0;
}

static size_t fs_total_bytes(void)
{
    size_t total = 0, used = 0;
    esp_littlefs_info(SPOOL_LABEL, &total, &used);
    return total;
}

/* ---- metadata ---------------------------------------------------------- */

/*
 * The `capture` object the grant request carries. Everything the board could
 * not read is omitted rather than sent as a zero or a guess - a missing field
 * is honest, a fabricated one is a lie the analysis downstream cannot see
 * through.
 *
 * captured_at is NOT written here: an entry captured before the clock synced
 * does not have one yet, so it is spliced in when the entry is uploaded.
 */
static int build_meta(char *out, size_t cap, mirror_trigger_t trigger)
{
    board_camera_capture_info_t info;
    board_camera_capture_info(&info);
    const esp_app_desc_t *app = esp_app_get_description();

    int n = snprintf(out, cap, "{\"firmware_version\":\"%s\"",
                     app && app->version[0] ? app->version : CONFIG_MIRROR_FW_VERSION);
    if (info.sensor && info.sensor[0]) {
        n += snprintf(out + n, cap - n, ",\"sensor\":\"%s\"", info.sensor);
    }
    if (info.width && info.height) {
        n += snprintf(out + n, cap - n, ",\"width\":%" PRIu32 ",\"height\":%" PRIu32,
                      info.width, info.height);
    }
    if (info.jpeg_quality >= 0) {
        n += snprintf(out + n, cap - n, ",\"jpeg_quality\":%d", info.jpeg_quality);
    }
    if (info.exposure_us >= 0) {
        n += snprintf(out + n, cap - n, ",\"exposure_us\":%" PRId32, info.exposure_us);
    }
    if (info.analog_gain >= 0.0f) {
        n += snprintf(out + n, cap - n, ",\"analog_gain\":%.3f", info.analog_gain);
    }
    if (info.mean_luma >= 0) {
        n += snprintf(out + n, cap - n, ",\"mean_luma\":%d", info.mean_luma);
    }
    if (info.af_state && info.af_state[0]) {
        n += snprintf(out + n, cap - n, ",\"af_state\":\"%s\"", info.af_state);
    }
    if (info.focus_score >= 0) {
        n += snprintf(out + n, cap - n, ",\"focus_score\":%d", info.focus_score);
    }
    if (board_flash_available()) {
        n += snprintf(out + n, cap - n, ",\"flash\":%s", info.flash ? "true" : "false");
    }
    n += snprintf(out + n, cap - n, ",\"trigger\":\"%s\",\"capture_source\":\"device\"}",
                  mirror_trigger_name(trigger));
    return n;
}

/*
 * Splice captured_at into the stored object. The stored
 * form always ends in '}', so this is a string edit rather than a JSON
 * round-trip - which would cost several kilobytes of internal heap on the
 * upload path, the one place this firmware has none to spare.
 */
static void meta_with_time(char *out, size_t cap, const char *stored,
                           time_t captured_at)
{
    struct tm utc;
    char stamp[24];
    gmtime_r(&captured_at, &utc);
    strftime(stamp, sizeof(stamp), "%Y-%m-%dT%H:%M:%SZ", &utc);

    size_t n = strlen(stored);
    if (n < 2 || stored[n - 1] != '}') {
        snprintf(out, cap, "{\"captured_at\":\"%s\"}", stamp);
        return;
    }
    snprintf(out, cap, "%.*s,\"captured_at\":\"%s\"}", (int)(n - 1), stored, stamp);
}

const char *mirror_spool_last_meta(void) { return s_last_meta; }

/*
 * `YYYYMMDDTHHMMSSZ-<8 hex>`, the format the Pi has always used
 * (device/src/main.rs `capture_id`).
 *
 * It is not cosmetic. The server slices the ID apart to fill
 * photos.captured_at (server/src/catalog.rs `id_to_timestamp`, which reads
 * bytes 0..15), so the gallery's ordering and every date-based view are
 * downstream of this string. The random suffix makes a collision impossible
 * even within the same second.
 */
static void capture_id_for(char *out, size_t cap, time_t when, uint32_t seq)
{
    /*
     * The suffix is derived, not random, so an entry that is retried - or that
     * is still queued after a reboot - is filed under the SAME id every time.
     * A fresh random id per attempt looked harmless and is not: each attempt
     * reserves another catalog row and leaves another object in R2, so one
     * photo sitting behind a fifteen-minute backoff quietly becomes a dozen
     * orphans. FNV-1a over the device id and the sequence number: unique
     * across devices, unique across photos, and the same on every retry.
     */
    char device_id[13];
    mirror_device_id(device_id);
    uint32_t h = 2166136261u;
    for (const char *c = device_id; *c; c++) {
        h = (h ^ (uint8_t)*c) * 16777619u;
    }
    for (int i = 0; i < 4; i++) {
        h = (h ^ (uint8_t)(seq >> (i * 8))) * 16777619u;
    }

    struct tm utc;
    gmtime_r(&when, &utc);
    size_t stamped = strftime(out, cap, "%Y%m%dT%H%M%SZ", &utc);
    snprintf(out + stamped, cap - stamped, "-%08" PRIx32, h);
}

/* ---- adding ------------------------------------------------------------ */

esp_err_t mirror_spool_add(const uint8_t *jpeg, size_t len, mirror_trigger_t trigger)
{
    if (!s_mounted) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!jpeg || !len) {
        return ESP_ERR_INVALID_ARG;
    }

    char meta[320];
    int meta_len = build_meta(meta, sizeof(meta), trigger);
    if (meta_len < 0 || (size_t)meta_len >= sizeof(meta)) {
        return ESP_ERR_INVALID_SIZE;
    }
    snprintf(s_last_meta, sizeof(s_last_meta), "%s", meta);

    lock();

    /* Make room. Dropping the oldest is the right end to drop from: the newest
     * photo is the one someone just took and is standing there waiting for. */
    size_t need = sizeof(entry_hdr_t) + (size_t)meta_len + len + RESERVE_BYTES;
    while (fs_free_bytes() < need) {
        uint32_t lo = oldest_seq();
        if (!lo) {
            break;   /* nothing left to drop and it still does not fit */
        }
        ESP_LOGW(TAG, "spool full - dropping the oldest entry %010" PRIu32, lo);
        delete_seq(SPOOL_Q, lo);
        s_drops++;
        rescan(NULL, NULL);
    }
    if (fs_free_bytes() < need) {
        unlock();
        ESP_LOGE(TAG, "photo does not fit in the spool at all (%u bytes)", (unsigned)len);
        return ESP_ERR_NO_MEM;
    }

    entry_hdr_t hdr = {
        .magic = ENTRY_MAGIC,
        .version = ENTRY_VERSION,
        .seq = s_next_seq,
        .meta_len = (uint32_t)meta_len,
        .jpeg_len = (uint32_t)len,
        .captured_at = mirror_net_clock_valid() ? (int64_t)time(NULL) : 0,
        .flags = 0,
        .jpeg_offset = (uint32_t)(sizeof(entry_hdr_t) + (size_t)meta_len),
    };


    /* Temp name then rename: a power cut halfway through leaves tmp.dm, which
     * start-up deletes, rather than a half photo in the queue. */
    unlink(SPOOL_TMP);
    FILE *f = fopen(SPOOL_TMP, "wb");
    if (!f) {
        unlock();
        ESP_LOGE(TAG, "cannot open the spool temp file: %s", strerror(errno));
        return ESP_FAIL;
    }
    bool ok = fwrite(&hdr, 1, sizeof(hdr), f) == sizeof(hdr)
           && fwrite(meta, 1, (size_t)meta_len, f) == (size_t)meta_len
           && fwrite(jpeg, 1, len, f) == len;
    ok = (fflush(f) == 0) && ok;
    fclose(f);
    if (!ok) {
        unlink(SPOOL_TMP);
        unlock();
        ESP_LOGE(TAG, "spool write failed: %s", strerror(errno));
        return ESP_FAIL;
    }

    char dest[64];
    path_for(dest, sizeof(dest), SPOOL_Q, s_next_seq);
    if (rename(SPOOL_TMP, dest) != 0) {
        unlink(SPOOL_TMP);
        unlock();
        ESP_LOGE(TAG, "spool rename failed: %s", strerror(errno));
        return ESP_FAIL;
    }
    s_next_seq++;
    rescan(NULL, NULL);
    uint32_t count = s_count;
    unlock();

    ESP_LOGI(TAG, "spooled %010" PRIu32 " (%u bytes, %s) - %" PRIu32 " waiting",
             hdr.seq, (unsigned)len, mirror_trigger_name(trigger), count);

    /* Wake the drain task: it may be parked on a long backoff that the arrival
     * of a new photo has no reason to wait out. */
    if (s_task) {
        xTaskNotifyGive(s_task);
    }
    return ESP_OK;
}

/* ---- draining ---------------------------------------------------------- */

static void note_error(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(s_last_error, sizeof(s_last_error), fmt, ap);
    va_end(ap);
}

static void backoff_advance(void)
{
    if (s_backoff_ms == 0) {
        s_backoff_ms = BACKOFF_MIN_MS;
    } else {
        s_backoff_ms *= 2;
        if (s_backoff_ms > BACKOFF_MAX_MS) {
            s_backoff_ms = BACKOFF_MAX_MS;
        }
    }
    /* +/- 20% of jitter. Several devices in one house come back from a router
     * reboot at the same instant otherwise, and hit the server together. */
    int32_t span = (int32_t)(s_backoff_ms / 5);
    int32_t jitter = span ? (int32_t)(esp_random() % (uint32_t)(2 * span)) - span : 0;
    int64_t wait_ms = (int64_t)s_backoff_ms + jitter;
    if (wait_ms < 1000) {
        wait_ms = 1000;
    }
    s_next_retry_us = esp_timer_get_time() + wait_ms * 1000;
    ESP_LOGW(TAG, "upload retry in %" PRId64 " s (%s)", wait_ms / 1000, s_last_error);
}

static void backoff_reset(const char *why)
{
    if (s_backoff_ms || s_next_retry_us) {
        ESP_LOGI(TAG, "retry backoff reset (%s)", why);
    }
    s_backoff_ms = 0;
    s_next_retry_us = 0;
}

/* Keep at most REJECTED_MAX files in r/, dropping the oldest. */
static void reject_entry(uint32_t seq)
{
    char src[64], dst[64];
    path_for(src, sizeof(src), SPOOL_Q, seq);
    path_for(dst, sizeof(dst), SPOOL_R, seq);

    /* Prune first, so moving one in never overruns the cap. */
    for (;;) {
        DIR *d = opendir(SPOOL_R);
        if (!d) {
            break;
        }
        uint32_t n = 0, lo = UINT32_MAX;
        struct dirent *e;
        while ((e = readdir(d)) != NULL) {
            uint32_t s = (uint32_t)strtoul(e->d_name, NULL, 10);
            n++;
            if (s < lo) lo = s;
        }
        closedir(d);
        if (n < REJECTED_MAX || lo == UINT32_MAX) {
            break;
        }
        delete_seq(SPOOL_R, lo);
    }

    if (rename(src, dst) != 0) {
        unlink(src);
    }
    s_rejected++;
    rescan(NULL, NULL);
}

/*
 * What captured_at should be for this entry.
 *
 * Normally it is the moment of the shutter, recorded when the photo was
 * spooled. A photo taken before SNTP had answered has no honest timestamp at
 * all, so it carries a zero and is simply held - and then stamped with the
 * time at which the clock first became valid. That is a few seconds to a few
 * minutes late in the only case it can happen (a press in the window between
 * boot and the first SNTP reply), and it is a real time rather than a
 * reconstruction. Queue order is capture order either way.
 *
 * Returns false while there is no clock at all, which is what holds the photo.
 */
static bool resolve_time(const entry_hdr_t *hdr, time_t *out)
{
    if (hdr->captured_at > 0) {
        *out = (time_t)hdr->captured_at;
        return true;
    }
    if (!mirror_net_clock_valid()) {
        return false;
    }
    *out = time(NULL);
    return true;
}

/* One entry. Returns true if the caller should immediately try the next. */
static bool drain_one(void)
{
    lock();
    uint32_t seq = oldest_seq();
    unlock();
    if (!seq) {
        return false;
    }

    char path[64];
    path_for(path, sizeof(path), SPOOL_Q, seq);

    entry_hdr_t hdr;
    FILE *f = NULL;
    if (!read_header(path, &hdr, &f)) {
        ESP_LOGW(TAG, "unreadable spool entry %010" PRIu32 " - discarding", seq);
        lock();
        unlink(path);
        rescan(NULL, NULL);
        unlock();
        return true;
    }

    /* Before anything else: is this actually a whole JPEG? A misaligned or
     * truncated one is not worth a fifteen-minute retry cycle, and it is not
     * worth the server's time either. */
    if (!jpeg_intact(f, &hdr)) {
        fclose(f);
        note_error("entry %010" PRIu32 " is not a complete JPEG (no SOI/EOI)", seq);
        ESP_LOGE(TAG, "%s - moving it aside", s_last_error);
        lock();
        reject_entry(seq);
        unlock();
        return true;
    }

    /* The metadata lives between the header and the JPEG. read_header() left
     * the handle on the JPEG, so this reads from its own offset and puts it
     * back - the whole point of storing jpeg_offset is that nothing has to
     * reason about where one section ends and the next begins. */
    char stored[320] = "";
    if (hdr.meta_len && hdr.meta_len < sizeof(stored)) {
        long resume = ftell(f);
        if (fseek(f, (long)(hdr.jpeg_offset - hdr.meta_len), SEEK_SET) == 0
            && fread(stored, 1, hdr.meta_len, f) == hdr.meta_len) {
            stored[hdr.meta_len] = '\0';
        } else {
            stored[0] = '\0';
        }
        if (fseek(f, resume, SEEK_SET) != 0) {
            fclose(f);
            return false;
        }
    }

    time_t captured_at = 0;
    if (!resolve_time(&hdr, &captured_at)) {
        fclose(f);
        return false;   /* held until the clock syncs */
    }

    char meta[384];
    meta_with_time(meta, sizeof(meta), stored, captured_at);

    /* An entry spooled before the clock synced gets its timestamp now, written
     * back so a retry does not decide a different one and file the photo
     * twice. Only the captured_at field is rewritten, at its own offset -
     * writing the whole struct would be exactly the mistake that produced the
     * misaligned uploads, since a version-1 entry's header on disk is a
     * different length from this one. */
    if (hdr.captured_at <= 0) {
        hdr.captured_at = (int64_t)captured_at;
        long resume = ftell(f);
        fclose(f);
        f = NULL;
        lock();
        FILE *w = fopen(path, "r+b");
        if (w) {
            if (fseek(w, (long)offsetof(entry_hdr_t, captured_at), SEEK_SET) == 0) {
                fwrite(&hdr.captured_at, 1, sizeof(hdr.captured_at), w);
            }
            fclose(w);
        }
        unlock();
        f = fopen(path, "rb");
        if (!f || fseek(f, resume, SEEK_SET) != 0) {
            if (f) {
                fclose(f);
            }
            return false;
        }
    }

    char capture_id[48];
    capture_id_for(capture_id, sizeof(capture_id), captured_at, seq);

    ESP_LOGI(TAG, "uploading %010" PRIu32 " as %s (%" PRIu32 " bytes)",
             seq, capture_id, hdr.jpeg_len);
    ESP_LOGD(TAG, "capture meta %s", meta);

    /* The file is already positioned at the first JPEG byte. */
    mirror_upload_req_t req = {
        .capture_id = capture_id,
        .meta_json = meta,
        .f = f,
        .len = hdr.jpeg_len,
    };

    s_uploading = true;
    int status = 0;
    int64_t t0 = esp_timer_get_time();
    esp_err_t err = mirror_upload_file(&req, &status);
    int64_t took_ms = (esp_timer_get_time() - t0) / 1000;
    s_uploading = false;
    fclose(f);

    s_last_http_status = status;

    if (err == ESP_OK) {
        s_uploads_ok++;
        s_failed_seq = 0;
        backoff_reset("upload succeeded");
        note_error("%s", "");
        lock();
        unlink(path);
        rescan(NULL, NULL);
        unlock();
        ESP_LOGI(TAG, "uploaded %s in %" PRId64 " ms - %" PRIu32 " left",
                 capture_id, took_ms, s_count);
        return true;
    }

    s_uploads_failed++;
    note_error("%s", mirror_upload_last_result());

    /* Red once per photo, not once per attempt. A fifteen-minute backoff that
     * flashed red every time it woke up would turn a network outage into a
     * device that looks broken all evening. */
    if (s_failed_seq != seq) {
        s_failed_seq = seq;
        mirror_ring_layer_play(RING_LAYER_UPLOAD, RING_RED_TRIPLE_PULSE);
    }

    if (status == 401 || status == 403) {
        /* The credential is wrong. Every retry will fail the same way, and the
         * photos are not ours to delete - stop and make it visible. */
        s_token_bad = true;
        ESP_LOGE(TAG, "HTTP %d - the device token is not accepted. Draining stopped; "
                      "%" PRIu32 " photos are being kept.", status, s_count);
        return false;
    }
    if (status == 400 || status == 409 || status == 413) {
        ESP_LOGE(TAG, "HTTP %d for %010" PRIu32 " - this photo will never be "
                      "accepted; moving it aside", status, seq);
        lock();
        reject_entry(seq);
        unlock();
        return true;
    }

    backoff_advance();
    return false;
}

static bool ready_to_drain(void)
{
    if (s_token_bad || !s_mounted) {
        return false;
    }
    if (!mirror_upload_configured() || !mirror_net_sta_connected()) {
        return false;
    }
    if (!mirror_net_clock_valid()) {
        return false;   /* see the header: no honest timestamp, no upload */
    }
    if (s_next_retry_us && esp_timer_get_time() < s_next_retry_us) {
        return false;
    }
    return s_count > 0;
}

/*
 * What the ring should say about the spool, on its own layer.
 *
 * Green slow pulse for "there is work in hand" - which covers the photo on
 * the wire and the backlog waiting behind a backoff equally, because from
 * across the room they are the same fact. Three green flashes exactly once,
 * when the last one goes. Nothing at all when the queue is empty, so the
 * layer underneath (idle, or pairing) shows through.
 */
static void update_ring(void)
{
    static bool had_work;
    bool work = s_mounted && s_count > 0 && !s_token_bad;

    if (work) {
        mirror_ring_layer_set(RING_LAYER_UPLOAD, RING_GREEN_SLOW_PULSE);
    } else if (had_work) {
        /* Blocking, on this task, which is exactly where it belongs - the
         * press flow must never wait behind it. */
        mirror_ring_layer_play(RING_LAYER_UPLOAD, RING_GREEN_TRIPLE_FLASH);
    } else {
        mirror_ring_layer_clear(RING_LAYER_UPLOAD);
    }
    had_work = work;
}

static void drain_task(void *arg)
{
    (void)arg;
    for (;;) {
        update_ring();
        if (ready_to_drain()) {
            if (!drain_one()) {
                /* Either the queue emptied or we backed off; either way, sleep
                 * rather than spin. */
                update_ring();
                ulTaskNotifyTake(pdTRUE, pdMS_TO_TICKS(5000));
            }
        } else {
            ulTaskNotifyTake(pdTRUE, pdMS_TO_TICKS(5000));
        }
    }
}

/*
 * A new address means the thing that was almost certainly wrong - the network -
 * is right again. Waiting out the rest of a fifteen-minute backoff at that
 * point is just making the user wait for nothing.
 */
static void on_got_ip(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    (void)arg; (void)base; (void)data;
    if (id != IP_EVENT_STA_GOT_IP) {
        return;
    }
    backoff_reset("station got an address");
    if (s_task) {
        xTaskNotifyGive(s_task);
    }
}

/* ---- start-up ---------------------------------------------------------- */

esp_err_t mirror_spool_start(void)
{
    if (s_mounted) {
        return ESP_OK;
    }
    s_lock = xSemaphoreCreateMutex();
    if (!s_lock) {
        return ESP_ERR_NO_MEM;
    }
    esp_vfs_littlefs_conf_t conf = {
        .base_path = SPOOL_BASE,
        .partition_label = SPOOL_LABEL,
        .format_if_mount_failed = true,
        .dont_mount = false,
    };
    esp_err_t err = esp_vfs_littlefs_register(&conf);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "spool partition would not mount: %s - captures will not "
                      "be kept across a failed upload", esp_err_to_name(err));
        return err;
    }
    s_mounted = true;

    mkdir(SPOOL_Q, 0755);
    mkdir(SPOOL_R, 0755);
    /* Whatever a power cut left half-written. */
    unlink(SPOOL_TMP);

    uint32_t lo = 0, hi = 0;
    rescan(&lo, &hi);
    s_next_seq = hi + 1;

    size_t total = 0, used = 0;
    esp_littlefs_info(SPOOL_LABEL, &total, &used);
    ESP_LOGI(TAG, "spool mounted: %u KB total, %u KB used, %" PRIu32 " queued "
                  "(seq %" PRIu32 "..%" PRIu32 ")",
             (unsigned)(total / 1024), (unsigned)(used / 1024), s_count, lo, hi);

    esp_event_handler_instance_register(IP_EVENT, IP_EVENT_STA_GOT_IP,
                                        &on_got_ip, NULL, NULL);

    /* 10 KB. The upload runs on this task, and mbedTLS' handshake is most of
     * it - 6 KB overflowed on the first real upload, which on this chip means
     * an immediate reboot and, because the entry is still queued, a boot loop.
     * The button task is 8 KB for the same handshake; this one carries the
     * LittleFS read path on top. */
    if (xTaskCreate(drain_task, "spool", 10240, NULL, 4, &s_task) != pdPASS) {
        ESP_LOGE(TAG, "could not start the drain task");
        return ESP_ERR_NO_MEM;
    }
    return ESP_OK;
}

uint32_t mirror_spool_count(void)  { return s_count; }
bool mirror_spool_uploading(void)  { return s_uploading; }

int mirror_spool_stats(char *buf, size_t cap)
{
    uint32_t oldest_age = 0;
    if (s_mounted && s_count) {
        lock();
        uint32_t lo = oldest_seq();
        unlock();
        if (lo) {
            char p[64];
            path_for(p, sizeof(p), SPOOL_Q, lo);
            entry_hdr_t hdr;
            if (read_header(p, &hdr, NULL) && hdr.captured_at > 0
                && mirror_net_clock_valid()) {
                int64_t age = (int64_t)time(NULL) - hdr.captured_at;
                oldest_age = age > 0 ? (uint32_t)age : 0;
            }
        }
    }

    int64_t next_retry_s = 0;
    if (s_next_retry_us) {
        int64_t d = (s_next_retry_us - esp_timer_get_time()) / 1000000;
        next_retry_s = d > 0 ? d : 0;
    }

    return snprintf(buf, cap,
        "spool_mounted=%d\n"
        "spool_count=%" PRIu32 "\n"
        "spool_bytes=%" PRIu64 "\n"
        "spool_capacity=%u\n"
        "spool_free=%u\n"
        "oldest_age_s=%" PRIu32 "\n"
        "uploads_ok=%" PRIu32 "\n"
        "uploads_failed=%" PRIu32 "\n"
        "drops=%" PRIu32 "\n"
        "rejected=%" PRIu32 "\n"
        "next_retry_s=%" PRId64 "\n"
        "last_http_status=%d\n"
        "last_error=%s\n"
        "token_bad=%d\n"
        "uploading=%d\n"
        "upload_blocked=%d\n"
        "last_capture=%s\n",
        s_mounted ? 1 : 0, s_count, s_bytes,
        (unsigned)fs_total_bytes(), (unsigned)fs_free_bytes(),
        oldest_age, s_uploads_ok, s_uploads_failed, s_drops, s_rejected,
        next_retry_s, s_last_http_status, s_last_error,
        s_token_bad ? 1 : 0, s_uploading ? 1 : 0,
        mirror_upload_blocked() ? 1 : 0,
        s_last_meta[0] ? s_last_meta : "none yet");
}
