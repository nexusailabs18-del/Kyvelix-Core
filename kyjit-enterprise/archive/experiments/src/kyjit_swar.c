#include <stdint.h>
#include <stddef.h>
#include "kyjit_swar.h"

/* SWAR popcount: count set bits in 8 bytes at once using 64-bit tricks */
uint64_t kyjit_swar_popcount(const uint8_t *data, size_t len) {
    uint64_t count = 0;
    size_t i = 0;
    
    /* Process 8 bytes per iteration using bit hacks */
    for(; i + 8 <= len; i += 8) {
        uint64_t v;
        memcpy(&v, &data[i], 8);
        /* SWAR popcount: count bits in 64-bit word */
        v = v - ((v >> 1) & 0x5555555555555555ULL);
        v = (v & 0x3333333333333333ULL) + ((v >> 2) & 0x3333333333333333ULL);
        v = (v + (v >> 4)) & 0x0F0F0F0F0F0F0F0FULL;
        count += (v * 0x0101010101010101ULL) >> 56;
    }
    /* Tail */
    for(; i < len; i++) {
        count += __builtin_popcount(data[i]);
    }
    return count;
}

/* SWAR sum of bytes: add 8 bytes at once using 64-bit arithmetic */
uint64_t kyjit_swar_bytesum(const uint8_t *data, size_t len) {
    uint64_t sum = 0;
    size_t i = 0;
    for(; i + 8 <= len; i += 8) {
        uint64_t v;
        memcpy(&v, &data[i], 8);
        /* Sum bytes using SWAR: prevent carry between bytes */
        uint64_t masked = v & 0x00FF00FF00FF00FFULL;
        uint64_t shifted = (v >> 8) & 0x00FF00FF00FF00FFULL;
        uint64_t s = masked + shifted;
        /* Accumulate into 64-bit total (no overflow within 64-bit for small arrays) */
        sum += (s & 0xFFFF) + (s >> 16);
    }
    for(; i < len; i++) sum += data[i];
    return sum;
}
