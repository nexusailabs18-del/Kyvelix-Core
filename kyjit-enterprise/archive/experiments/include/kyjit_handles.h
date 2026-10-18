#ifndef KYJIT_HANDLES_H
#define KYJIT_HANDLES_H

#include <stdint.h>
#include <stddef.h>

typedef struct KyjitHandleTable KyjitHandleTable;
typedef uint32_t KyjitHandle;

#define KYJIT_INVALID_HANDLE ((KyjitHandle)0xFFFFFFFF)

KyjitHandleTable *kyjit_handle_table_create(size_t initial_capacity);
void             *kyjit_handle_get(KyjitHandleTable *t, KyjitHandle handle);
KyjitHandle       kyjit_handle_create(KyjitHandleTable *t, void *ptr);
void              kyjit_handle_destroy(KyjitHandleTable *t, KyjitHandle handle);
void              kyjit_handle_clear(KyjitHandleTable *t);
void              kyjit_handle_table_free(KyjitHandleTable *t);
size_t            kyjit_handle_count(KyjitHandleTable *t);

#endif
