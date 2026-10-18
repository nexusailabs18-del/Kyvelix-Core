#ifndef KYJIT_BASE64_H
#define KYJIT_BASE64_H
#include <stdint.h>
#include <stddef.h>
void kyjit_base64_init(void);
size_t kyjit_base64_encode_lut(const uint8_t *src, size_t src_len, char *dst);
#endif
