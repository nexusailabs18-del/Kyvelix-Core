#ifndef KYJIT_CRASH_H
#define KYJIT_CRASH_H
#include <stdio.h>
#include <signal.h>
#include <stdlib.h>
#include <stdint.h>

typedef void (*KyCrashHandler)(int sig, uint64_t fault_addr);

static inline void ky_crash_register(KyCrashHandler handler) {
    if (handler) {
        signal(SIGSEGV, (void (*)(int))handler);
    }
}
#endif
