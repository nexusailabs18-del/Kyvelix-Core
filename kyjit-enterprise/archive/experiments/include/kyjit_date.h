#ifndef KYJIT_DATE_H
#define KYJIT_DATE_H

#include <stdint.h>
#include <stddef.h>

/* Millisecond timestamp */
typedef int64_t KyjitTimestamp;

/* Get current time in milliseconds */
KyjitTimestamp kyjit_date_now_ms(void);

/* Parse ISO 8601 date "2026-08-27T18:30:00" → milliseconds */
KyjitTimestamp kyjit_date_parse_iso(const char *iso);

/* Format milliseconds to ISO 8601 */
int kyjit_date_format_iso(KyjitTimestamp ts, char *buf, size_t buflen);

/* Get year, month, day, hour, min, sec from timestamp */
void kyjit_date_breakdown(KyjitTimestamp ts,
                          int *year, int *month, int *day,
                          int *hour, int *min, int *sec);

/* Convert year/month/day to timestamp (UTC) */
KyjitTimestamp kyjit_date_make(int year, int month, int day,
                                int hour, int min, int sec);

#endif
