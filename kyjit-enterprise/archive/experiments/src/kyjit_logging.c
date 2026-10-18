#include "kyjit_logging.h"
#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <string.h>
#include <time.h>
#include <pthread.h>

static int current_level = KYJIT_LOG_INFO;
static FILE *log_file = NULL;
static pthread_mutex_t log_mutex = PTHREAD_MUTEX_INITIALIZER;

static const char *level_name(int level) {
    switch (level) {
        case KYJIT_LOG_DEBUG: return "DEBUG";
        case KYJIT_LOG_INFO:  return "INFO";
        case KYJIT_LOG_WARN:  return "WARN";
        case KYJIT_LOG_ERROR: return "ERROR";
        case KYJIT_LOG_FATAL: return "FATAL";
        default: return "?";
    }
}

void kyjit_log_set_level(int level) {
    if (level < KYJIT_LOG_DEBUG) level = KYJIT_LOG_DEBUG;
    if (level > KYJIT_LOG_FATAL) level = KYJIT_LOG_FATAL;
    current_level = level;
}

int kyjit_log_set_file(const char *path) {
    FILE *f = fopen(path, "a");
    if (!f) return -1;

    pthread_mutex_lock(&log_mutex);
    if (log_file) fclose(log_file);
    log_file = f;
    pthread_mutex_unlock(&log_mutex);
    return 0;
}

void kyjit_log_impl(int level, const char *file, int line, const char *fmt, ...) {
    if (level < current_level) return;

    pthread_mutex_lock(&log_mutex);

    time_t now = time(NULL);
    struct tm tm_buf;
    localtime_r(&now, &tm_buf);
    char time_buf[32];
    strftime(time_buf, sizeof(time_buf), "%Y-%m-%d %H:%M:%S", &tm_buf);

    FILE *out = log_file ? log_file : stderr;
    fprintf(out, "[%s][%s][%s:%d] ",
            time_buf, level_name(level), file, line);

    va_list ap;
    va_start(ap, fmt);
    vfprintf(out, fmt, ap);
    va_end(ap);
    fputc('\n', out);
    fflush(out);

    pthread_mutex_unlock(&log_mutex);

    if (level == KYJIT_LOG_FATAL) abort();
}
