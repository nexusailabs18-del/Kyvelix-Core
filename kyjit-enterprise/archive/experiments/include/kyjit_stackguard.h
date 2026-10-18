#ifndef KYJIT_STACKGUARD_H
#define KYJIT_STACKGUARD_H

#include <stddef.h>

typedef struct stack_guard stack_guard_t;

stack_guard_t* kyjit_stackguard_create(size_t stack_size);
int kyjit_stackguard_check(stack_guard_t *sg);
void kyjit_stackguard_destroy(stack_guard_t *sg);
int kyjit_stackguard_available(void);

#endif
