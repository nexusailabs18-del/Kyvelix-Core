#ifndef KYJIT_DWARF_H
#define KYJIT_DWARF_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    uint64_t pc;
    uint32_t line;
    uint32_t column;
    char function[128];
} KyDwarfEntry;

typedef struct {
    KyDwarfEntry *entries;
    uint32_t count;
    uint32_t capacity;
} KyDwarf;

static inline KyDwarf *ky_dwarf_create(uint32_t capacity) {
    KyDwarf *d = calloc(1, sizeof(*d));
    if (!d) return NULL;
    d->entries = calloc(capacity, sizeof(KyDwarfEntry));
    d->capacity = capacity;
    return d;
}

static inline void ky_dwarf_add_entry(KyDwarf *d, uint64_t pc, uint32_t line, uint32_t column, const char *func) {
    if (!d || d->count >= d->capacity) return;
    d->entries[d->count].pc = pc;
    d->entries[d->count].line = line;
    d->entries[d->count].column = column;
    strncpy(d->entries[d->count].function, func, 127);
    d->count++;
}

static inline KyDwarfEntry *ky_dwarf_lookup(KyDwarf *d, uint64_t pc) {
    if (!d) return NULL;
    for (uint32_t i = 0; i < d->count; i++) {
        if (d->entries[i].pc == pc) return &d->entries[i];
    }
    return NULL;
}
#endif
