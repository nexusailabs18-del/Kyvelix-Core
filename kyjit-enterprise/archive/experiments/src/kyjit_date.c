#include "kyjit_date.h"
#include <stdio.h>
#include <string.h>
#include <time.h>

KyjitTimestamp kyjit_date_now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    return (KyjitTimestamp)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

KyjitTimestamp kyjit_date_parse_iso(const char *iso) {
    int year, month, day, hour, min, sec;
    if (sscanf(iso, "%d-%d-%dT%d:%d:%d",
               &year, &month, &day, &hour, &min, &sec) != 6) {
        return -1;
    }
    return kyjit_date_make(year, month, day, hour, min, sec);
}

int kyjit_date_format_iso(KyjitTimestamp ts, char *buf, size_t buflen) {
    int year, month, day, hour, min, sec;
    kyjit_date_breakdown(ts, &year, &month, &day, &hour, &min, &sec);
    return snprintf(buf, buflen, "%04d-%02d-%02dT%02d:%02d:%02d",
                    year, month, day, hour, min, sec);
}

void kyjit_date_breakdown(KyjitTimestamp ts,
                          int *year, int *month, int *day,
                          int *hour, int *min, int *sec) {
    time_t seconds = ts / 1000;
    struct tm tm_buf;
    gmtime_r(&seconds, &tm_buf);

    *year  = tm_buf.tm_year + 1900;
    *month = tm_buf.tm_mon + 1;
    *day   = tm_buf.tm_mday;
    *hour  = tm_buf.tm_hour;
    *min   = tm_buf.tm_min;
    *sec   = tm_buf.tm_sec;
}

KyjitTimestamp kyjit_date_make(int year, int month, int day,
                                int hour, int min, int sec) {
    struct tm tm_buf;
    memset(&tm_buf, 0, sizeof(tm_buf));
    tm_buf.tm_year = year - 1900;
    tm_buf.tm_mon  = month - 1;
    tm_buf.tm_mday = day;
    tm_buf.tm_hour = hour;
    tm_buf.tm_min  = min;
    tm_buf.tm_sec  = sec;

    time_t t = timegm(&tm_buf);
    return (KyjitTimestamp)t * 1000;
}
