#ifndef KYJIT_ROOTS_H
#define KYJIT_ROOTS_H

#include <stddef.h>

typedef struct KyjitRootSet KyjitRootSet;

KyjitRootSet *kyjit_roots_create(size_t initial_capacity);
int           kyjit_roots_add(KyjitRootSet *rs, void *ptr);
int           kyjit_roots_remove(KyjitRootSet *rs, void *ptr);
void          kyjit_roots_clear(KyjitRootSet *rs);
size_t        kyjit_roots_count(KyjitRootSet *rs);
void         *kyjit_roots_get(KyjitRootSet *rs, size_t index);
void          kyjit_roots_free(KyjitRootSet *rs);

#endif
