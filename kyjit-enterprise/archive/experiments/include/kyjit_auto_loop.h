#ifndef KYJIT_AUTO_LOOP_H
#define KYJIT_AUTO_LOOP_H
#include <stdint.h>

typedef enum {
    PATTERN_NONE = 0,
    PATTERN_SUM_SEQUENTIAL,
    PATTERN_SUM_SQUARES,
    PATTERN_COUNT
} KyjitPattern;

KyjitPattern kyjit_detect_pattern(const char *code);
int64_t kyjit_execute_pattern(KyjitPattern p, int64_t n);

#endif
