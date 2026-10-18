#ifndef KYJIT_LOGGING_H
#define KYJIT_LOGGING_H

#define KYJIT_LOG_DEBUG 0
#define KYJIT_LOG_INFO  1
#define KYJIT_LOG_WARN  2
#define KYJIT_LOG_ERROR 3
#define KYJIT_LOG_FATAL 4

void kyjit_log_set_level(int level);
int  kyjit_log_set_file(const char *path);
void kyjit_log_impl(int level, const char *file, int line, const char *fmt, ...);

#define KYJIT_LOG(level, ...) \
    kyjit_log_impl(level, __FILE__, __LINE__, __VA_ARGS__)

#define KYJIT_DEBUG(...) KYJIT_LOG(KYJIT_LOG_DEBUG, __VA_ARGS__)
#define KYJIT_INFO(...)  KYJIT_LOG(KYJIT_LOG_INFO, __VA_ARGS__)
#define KYJIT_WARN(...)  KYJIT_LOG(KYJIT_LOG_WARN, __VA_ARGS__)
#define KYJIT_ERROR(...) KYJIT_LOG(KYJIT_LOG_ERROR, __VA_ARGS__)
#define KYJIT_FATAL(...) KYJIT_LOG(KYJIT_LOG_FATAL, __VA_ARGS__)

#endif
