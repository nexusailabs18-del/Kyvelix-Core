#ifndef KYJIT_SWAR_H
#define KYJIT_SWAR_H
#include <stdint.h>
#include <stddef.h>
#include <string.h>

uint64_t kyjit_swar_popcount(const uint8_t *data, size_t len);
uint64_t kyjit_swar_bytesum(const uint8_t *data, size_t len);
#endif
