#ifndef KYJIT_SNAPSHOT_H
#define KYJIT_SNAPSHOT_H

#include <stdint.h>
#include <stddef.h>

typedef struct KyjitSnapshot KyjitSnapshot;

/* Create snapshot buffer */
KyjitSnapshot *kyjit_snapshot_create(void);

/* Serialize data into snapshot */
int kyjit_snapshot_add(KyjitSnapshot *snap, const void *data, size_t size);

/* Get snapshot data and size */
const void *kyjit_snapshot_data(KyjitSnapshot *snap);
size_t       kyjit_snapshot_size(KyjitSnapshot *snap);

/* Save snapshot to file */
int kyjit_snapshot_save(KyjitSnapshot *snap, const char *path);

/* Load snapshot from file */
KyjitSnapshot *kyjit_snapshot_load(const char *path);

/* Free snapshot */
void kyjit_snapshot_free(KyjitSnapshot *snap);

#endif
