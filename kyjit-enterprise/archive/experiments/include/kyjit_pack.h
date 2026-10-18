#ifndef KYJIT_PACK_H
#define KYJIT_PACK_H
#include <stdint.h>
void kyjit_unpack_8to32(const uint8_t *restrict packed, int32_t *restrict unpacked, int n);
void kyjit_unpack_16to32(const uint16_t *restrict packed, int32_t *restrict unpacked, int n);
#endif
