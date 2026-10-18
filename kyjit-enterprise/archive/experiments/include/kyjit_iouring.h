#ifndef KYJIT_IOURING_H
#define KYJIT_IOURING_H
#include <sys/types.h>

int kyjit_iouring_init(int queue_depth);
ssize_t kyjit_iouring_read_file(const char *path, void *buf, size_t size);
void kyjit_iouring_cleanup(void);

#endif
