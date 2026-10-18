#include <unistd.h>
#include "kyjit_trap.h"
#include <stdio.h>
#include <stdlib.h>
#include <signal.h>

static KyjitTrapHandler user_handler = NULL;
static KyjitTrapType last_type = KYJIT_TRAP_NONE;
static void *last_addr = NULL;

static void signal_handler(int sig, siginfo_t *info, void *ctx) {
    (void)ctx;

    switch (sig) {
        case SIGSEGV: last_type = KYJIT_TRAP_SEGV; break;
        case SIGILL:  last_type = KYJIT_TRAP_ILL;  break;
        case SIGFPE:  last_type = KYJIT_TRAP_FPE;  break;
        case SIGBUS:  last_type = KYJIT_TRAP_BUS;  break;
        default:      last_type = KYJIT_TRAP_NONE; break;
    }

    last_addr = info->si_addr;

    if (user_handler) {
        user_handler(last_type, last_addr);
    } else {
        fprintf(stderr, "KYJIT trap: type=%d addr=%p\n",
                last_type, last_addr);
        _exit(1);
    }
}

int kyjit_trap_install(KyjitTrapHandler handler) {
    user_handler = handler;

    struct sigaction sa;
    sa.sa_sigaction = signal_handler;
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = SA_SIGINFO;

    if (sigaction(SIGSEGV, &sa, NULL) != 0) return -1;
    if (sigaction(SIGILL,  &sa, NULL) != 0) return -1;
    if (sigaction(SIGFPE,  &sa, NULL) != 0) return -1;
    if (sigaction(SIGBUS,  &sa, NULL) != 0) return -1;

    return 0;
}

void kyjit_trap_uninstall(void) {
    signal(SIGSEGV, SIG_DFL);
    signal(SIGILL,  SIG_DFL);
    signal(SIGFPE,  SIG_DFL);
    signal(SIGBUS,  SIG_DFL);
    user_handler = NULL;
}

KyjitTrapType kyjit_trap_last_type(void) {
    return last_type;
}

void *kyjit_trap_last_address(void) {
    return last_addr;
}
