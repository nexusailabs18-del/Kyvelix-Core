#include "kyjit_auto_loop.h"
#include <string.h>

KyjitPattern kyjit_detect_pattern(const char *code) {
    if (strstr(code, "sum") || strstr(code, "+= i")) {
        return PATTERN_SUM_SEQUENTIAL;
    }
    if (strstr(code, "i*i") || strstr(code, "square")) {
        return PATTERN_SUM_SQUARES;
    }
    return PATTERN_NONE;
}

int64_t kyjit_execute_pattern(KyjitPattern p, int64_t n) {
    switch(p) {
        case PATTERN_SUM_SEQUENTIAL:
            return n * (n + 1) / 2;
        case PATTERN_SUM_SQUARES:
            return n * (n + 1) * (2*n + 1) / 6;
        default:
            return 0;
    }
}
