#include "kyjit_snapshot.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

struct KyjitSnapshot {
    uint8_t *data;
    size_t   size;
    size_t   capacity;
};

KyjitSnapshot *kyjit_snapshot_create(void) {
    KyjitSnapshot *snap = calloc(1, sizeof(*snap));
    snap->capacity = 4096;
    snap->data = malloc(snap->capacity);
    snap->size = 0;
    return snap;
}

int kyjit_snapshot_add(KyjitSnapshot *snap, const void *data, size_t size) {
    if (snap->size + size > snap->capacity) {
        size_t new_cap = snap->capacity * 2;
        while (new_cap < snap->size + size) new_cap *= 2;
        uint8_t *new_data = realloc(snap->data, new_cap);
        if (!new_data) return -1;
        snap->data = new_data;
        snap->capacity = new_cap;
    }
    memcpy(snap->data + snap->size, data, size);
    snap->size += size;
    return 0;
}

const void *kyjit_snapshot_data(KyjitSnapshot *snap) {
    return snap->data;
}

size_t kyjit_snapshot_size(KyjitSnapshot *snap) {
    return snap->size;
}

int kyjit_snapshot_save(KyjitSnapshot *snap, const char *path) {
    FILE *f = fopen(path, "wb");
    if (!f) return -1;

    size_t written = fwrite(snap->data, 1, snap->size, f);
    fclose(f);
    return (written == snap->size) ? 0 : -1;
}

KyjitSnapshot *kyjit_snapshot_load(const char *path) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;

    fseek(f, 0, SEEK_END);
    long fsize = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (fsize <= 0) {
        fclose(f);
        return NULL;
    }

    KyjitSnapshot *snap = kyjit_snapshot_create();
    kyjit_snapshot_add(snap, snap->data, 0);  /* reset */
    
    free(snap->data);
    snap->capacity = fsize;
    snap->size = fsize;
    snap->data = malloc(fsize);

    size_t read = fread(snap->data, 1, fsize, f);
    fclose(f);

    if (read != (size_t)fsize) {
        kyjit_snapshot_free(snap);
        return NULL;
    }
    return snap;
}

void kyjit_snapshot_free(KyjitSnapshot *snap) {
    if (!snap) return;
    free(snap->data);
    free(snap);
}
