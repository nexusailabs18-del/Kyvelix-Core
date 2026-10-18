#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>

#if __has_include(<liburing.h>)
#define KYJIT_HAS_IOURING 1
#include <liburing.h>
#else
#define KYJIT_HAS_IOURING 0
#endif

#include "kyjit_iouring.h"

#if KYJIT_HAS_IOURING
static struct io_uring ring;
static int ring_initialized = 0;
#endif

int kyjit_iouring_init(int queue_depth) {
#if KYJIT_HAS_IOURING
    if (ring_initialized) return 0;
    if (io_uring_queue_init(queue_depth, &ring, 0) < 0) return -1;
    ring_initialized = 1;
    return 0;
#else
    return -1;  /* Not available */
#endif
}

/* Read entire file with zero-copy (no kernel buffer copy) */
ssize_t kyjit_iouring_read_file(const char *path, void *buf, size_t size) {
#if KYJIT_HAS_IOURING
    struct io_uring_sqe *sqe;
    struct io_uring_cqe *cqe;
    
    int fd = open(path, O_RDONLY);
    if (fd < 0) return -1;
    
    sqe = io_uring_get_sqe(&ring);
    if (!sqe) { close(fd); return -1; }
    
    io_uring_prep_read(sqe, fd, buf, size, 0);
    io_uring_submit(&ring);
    io_uring_wait_cqe(&ring, &cqe);
    
    ssize_t ret = cqe->res;
    io_uring_cqe_seen(&ring, cqe);
    close(fd);
    return ret;
#else
    /* Fallback: standard read */
    int fd = open(path, O_RDONLY);
    if (fd < 0) return -1;
    ssize_t ret = read(fd, buf, size);
    close(fd);
    return ret;
#endif
}

void kyjit_iouring_cleanup(void) {
#if KYJIT_HAS_IOURING
    if (ring_initialized) {
        io_uring_queue_exit(&ring);
        ring_initialized = 0;
    }
#endif
}
