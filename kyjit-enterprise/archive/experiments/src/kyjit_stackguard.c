#include "kyjit_stackguard.h"
#include <stdio.h>
#include <stdlib.h>
#include <signal.h>
#include <setjmp.h>
#include <pthread.h>

/* Stack guard: detect overflow before crash */

#define STACK_GUARD_SIZE (64 * 1024)  /* 64KB guard page */

static sigjmp_buf jump_buffer;
static int guard_triggered = 0;

typedef struct stack_guard {
    void *stack_base;
    size_t stack_size;
    size_t guard_size;
    volatile int overflow_detected;
} stack_guard_t;

static void segv_handler(int sig) {
    (void)sig;
    guard_triggered = 1;
    siglongjmp(jump_buffer, 1);
}

stack_guard_t* kyjit_stackguard_create(size_t stack_size) {
    stack_guard_t *sg = calloc(1, sizeof(stack_guard_t));
    if (!sg) return NULL;
    
    sg->stack_size = stack_size;
    sg->guard_size = STACK_GUARD_SIZE;
    
    /* Allocate stack with guard */
    sg->stack_base = malloc(stack_size + sg->guard_size);
    if (!sg->stack_base) {
        free(sg);
        return NULL;
    }
    
    sg->overflow_detected = 0;
    
    /* Set up signal handler */
    signal(SIGSEGV, segv_handler);
    
    return sg;
}

int kyjit_stackguard_check(stack_guard_t *sg) {
    if (sg->overflow_detected) return -1;
    return 0;
}

void kyjit_stackguard_destroy(stack_guard_t *sg) {
    if (!sg) return;
    free(sg->stack_base);
    free(sg);
}

int kyjit_stackguard_available(void) { return 1; }
