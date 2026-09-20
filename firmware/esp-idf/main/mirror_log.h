/*
 * A log ring, and GET /logs.
 *
 * The serial console is the only record this firmware keeps of itself, and it
 * is only readable by someone standing next to the board with a USB cable. A
 * device on a shelf that stopped uploading four hours ago cannot be debugged
 * that way. So: tee the last ~8 KB of log text into PSRAM and serve it.
 *
 * "Tee", not "redirect" - esp_log_set_vprintf() replaces the sink, so the hook
 * below writes to the ring AND calls the original vprintf. Serial output is
 * unchanged.
 *
 * The ring is written from every task that logs, and the log macros are also
 * reachable from ISR context (esp_log has an early/ISR-safe path). A mutex is
 * therefore wrong twice over: it cannot be taken in an ISR, and a task holding
 * it while preempted would block every other logger. A short critical section
 * on a spinlock is the shape that works - the copy is a couple of hundred
 * bytes of memcpy - and ISR context is skipped entirely, because PSRAM is not
 * guaranteed reachable when the cache is disabled.
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include "esp_err.h"
#include "esp_http_server.h"

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Allocate the ring and install the vprintf hook. Call as early as possible -
 * before it returns, log lines are on the console only. Safe to call twice.
 *
 * Returns ESP_ERR_NO_MEM if PSRAM could not provide the ring, in which case
 * logging carries on exactly as before and /logs reports that it is off.
 */
esp_err_t mirror_log_init(void);

/** One line naming the firmware, why the chip last restarted, and the clock. */
void mirror_log_banner(void);

/**
 * Copy the ring, oldest line first, into `out` (NUL-terminated). Returns the
 * number of bytes written, not counting the terminator.
 */
size_t mirror_log_copy(char *out, size_t cap);

/** How many bytes the ring currently holds, and its capacity. */
void mirror_log_usage(size_t *used, size_t *capacity);

/** Register GET /logs on the debug server. */
esp_err_t mirror_log_register_http(httpd_handle_t server);

#ifdef __cplusplus
}
#endif
