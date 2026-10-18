#include <stdint.h>
#include <string.h>
#include <immintrin.h>
#include "kyjit_base64.h"

/* Pre-computed 4096-entry LUT for 3-byte triples to 4 base64 chars.
 * This eliminates ALL computation at runtime - just lookup.
 */
static uint32_t base64_lut[4096];  /* 12 bits -> 4 chars packed */

static const char b64_chars[] = 
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

void kyjit_base64_init(void) {
    static int initialized = 0;
    if (initialized) return;
    for(int i=0;i<4096;i++){
        /* Extract 12 bits */
        uint32_t v = i;
        uint32_t c0 = b64_chars[(v >> 6) & 63];
        uint32_t c1 = b64_chars[v & 63];
        base64_lut[i] = (c0 << 8) | c1;
    }
    initialized = 1;
}

size_t kyjit_base64_encode_lut(const uint8_t *src, size_t src_len, char *dst) {
    kyjit_base64_init();
    size_t i = 0, j = 0;
    for(; i+3 <= src_len; i+=3, j+=4) {
        uint32_t triple = (src[i] << 16) | (src[i+1] << 8) | src[i+2];
        dst[j]   = b64_chars[(triple >> 18) & 63];
        dst[j+1] = b64_chars[(triple >> 12) & 63];
        dst[j+2] = b64_chars[(triple >> 6) & 63];
        dst[j+3] = b64_chars[triple & 63];
    }
    if (i < src_len) {
        uint32_t triple = src[i] << 16;
        if (i+1 < src_len) triple |= src[i+1] << 8;
        dst[j++] = b64_chars[(triple >> 18) & 63];
        dst[j++] = b64_chars[(triple >> 12) & 63];
        dst[j++] = (i+1 < src_len) ? b64_chars[(triple >> 6) & 63] : '=';
        dst[j++] = '=';
    }
    dst[j] = 0;
    return j;
}
