#include "ky_codec.h"
#include <string.h>
#include <stdlib.h>

/*
 * KY Codec — Optimized for zero-copy and inline operations
 */

/* ---------- Initialization (Inline) ---------- */

void ky_codec_init_writer(ky_codec_ctx_t *ctx, uint8_t *buffer, size_t capacity) {
    if (!ctx) return;
    ctx->data = buffer;
    ctx->len = capacity;
    ctx->pos = 0;
    ctx->is_reader = 0;
}

void ky_codec_init_reader(ky_codec_ctx_t *ctx, const uint8_t *buffer, size_t len) {
    if (!ctx) return;
    ctx->data = (uint8_t *)buffer;
    ctx->len = len;
    ctx->pos = 0;
    ctx->is_reader = 1;
}

/* ---------- Write Functions (Inline-Optimized) ---------- */

int ky_codec_write_u8(ky_codec_ctx_t *ctx, uint8_t value) {
    if (!ctx || ctx->is_reader || ctx->pos >= ctx->len) return KY_CODEC_ERR_OVERFLOW;
    ctx->data[ctx->pos++] = value;
    return KY_CODEC_OK;
}

int ky_codec_write_u16(ky_codec_ctx_t *ctx, uint16_t value) {
    if (!ctx || ctx->is_reader || ctx->pos + 2 > ctx->len) return KY_CODEC_ERR_OVERFLOW;
    ctx->data[ctx->pos++] = (value >> 8) & 0xFF;
    ctx->data[ctx->pos++] = value & 0xFF;
    return KY_CODEC_OK;
}

int ky_codec_write_u32(ky_codec_ctx_t *ctx, uint32_t value) {
    if (!ctx || ctx->is_reader || ctx->pos + 4 > ctx->len) return KY_CODEC_ERR_OVERFLOW;
    ctx->data[ctx->pos++] = (value >> 24) & 0xFF;
    ctx->data[ctx->pos++] = (value >> 16) & 0xFF;
    ctx->data[ctx->pos++] = (value >> 8) & 0xFF;
    ctx->data[ctx->pos++] = value & 0xFF;
    return KY_CODEC_OK;
}

int ky_codec_write_bytes(ky_codec_ctx_t *ctx, const void *data, size_t len) {
    if (!ctx || ctx->is_reader || !data || ctx->pos + len > ctx->len) return KY_CODEC_ERR_OVERFLOW;
    memcpy(&ctx->data[ctx->pos], data, len);
    ctx->pos += len;
    return KY_CODEC_OK;
}

int ky_codec_write_string(ky_codec_ctx_t *ctx, const char *str) {
    if (!ctx || !str) return KY_CODEC_ERR_INVALID;
    size_t len = strlen(str);
    if (len > 0xFFFF) return KY_CODEC_ERR_OVERFLOW;
    
    int ret = ky_codec_write_u16(ctx, (uint16_t)len);
    if (ret != KY_CODEC_OK) return ret;
    return ky_codec_write_bytes(ctx, str, len);
}

int ky_codec_write_string_array(ky_codec_ctx_t *ctx, const char **strs, size_t count) {
    if (!ctx || !strs || count > 0xFFFF) return KY_CODEC_ERR_INVALID;
    
    int ret = ky_codec_write_u16(ctx, (uint16_t)count);
    if (ret != KY_CODEC_OK) return ret;
    
    for (size_t i = 0; i < count; i++) {
        ret = ky_codec_write_string(ctx, strs[i]);
        if (ret != KY_CODEC_OK) return ret;
    }
    return KY_CODEC_OK;
}

/* ---------- Read Functions (Zero-Copy) ---------- */

int ky_codec_read_u8(ky_codec_ctx_t *ctx, uint8_t *out) {
    if (!ctx || !ctx->is_reader || !out || ctx->pos >= ctx->len) return KY_CODEC_ERR_OVERFLOW;
    *out = ctx->data[ctx->pos++];
    return KY_CODEC_OK;
}

int ky_codec_read_u16(ky_codec_ctx_t *ctx, uint16_t *out) {
    if (!ctx || !ctx->is_reader || !out || ctx->pos + 2 > ctx->len) return KY_CODEC_ERR_OVERFLOW;
    *out = ((uint16_t)ctx->data[ctx->pos] << 8) | (uint16_t)ctx->data[ctx->pos + 1];
    ctx->pos += 2;
    return KY_CODEC_OK;
}

int ky_codec_read_u32(ky_codec_ctx_t *ctx, uint32_t *out) {
    if (!ctx || !ctx->is_reader || !out || ctx->pos + 4 > ctx->len) return KY_CODEC_ERR_OVERFLOW;
    *out = ((uint32_t)ctx->data[ctx->pos] << 24) |
           ((uint32_t)ctx->data[ctx->pos + 1] << 16) |
           ((uint32_t)ctx->data[ctx->pos + 2] << 8) |
           (uint32_t)ctx->data[ctx->pos + 3];
    ctx->pos += 4;
    return KY_CODEC_OK;
}

int ky_codec_read_bytes(ky_codec_ctx_t *ctx, void *buf, size_t len) {
    if (!ctx || !ctx->is_reader || !buf || ctx->pos + len > ctx->len) return KY_CODEC_ERR_OVERFLOW;
    memcpy(buf, &ctx->data[ctx->pos], len);
    ctx->pos += len;
    return KY_CODEC_OK;
}

int ky_codec_read_string(ky_codec_ctx_t *ctx, const char **str, size_t *len) {
    if (!ctx || !ctx->is_reader || !str || !len) return KY_CODEC_ERR_INVALID;
    
    uint16_t slen;
    int ret = ky_codec_read_u16(ctx, &slen);
    if (ret != KY_CODEC_OK) return ret;
    
    if (ctx->pos + slen > ctx->len) return KY_CODEC_ERR_OVERFLOW;
    
    /* Zero-copy: point directly into buffer */
    *str = (const char *)&ctx->data[ctx->pos];
    *len = slen;
    ctx->pos += slen;
    return KY_CODEC_OK;
}

int ky_codec_read_string_array(ky_codec_ctx_t *ctx, char ***strs, size_t **lengths, size_t *count) {
    if (!ctx || !ctx->is_reader || !strs || !count) return KY_CODEC_ERR_INVALID;
    
    uint16_t num;
    int ret = ky_codec_read_u16(ctx, &num);
    if (ret != KY_CODEC_OK) return ret;
    
    char **str_array = (char **)malloc(num * sizeof(char *));
    if (!str_array) return KY_CODEC_ERR_NOMEM;
    
    size_t *len_array = NULL;
    if (lengths) {
        len_array = (size_t *)malloc(num * sizeof(size_t));
        if (!len_array) {
            free(str_array);
            return KY_CODEC_ERR_NOMEM;
        }
    }
    
    for (uint16_t i = 0; i < num; i++) {
        const char *s;
        size_t l;
        ret = ky_codec_read_string(ctx, &s, &l);
        if (ret != KY_CODEC_OK) {
            for (uint16_t j = 0; j < i; j++) free(str_array[j]);
            free(str_array);
            if (len_array) free(len_array);
            return ret;
        }
        
        /* Copy string (caller expects mutable array) */
        char *copy = (char *)malloc(l + 1);
        if (!copy) {
            for (uint16_t j = 0; j < i; j++) free(str_array[j]);
            free(str_array);
            if (len_array) free(len_array);
            return KY_CODEC_ERR_NOMEM;
        }
        memcpy(copy, s, l);
        copy[l] = '\0';
        str_array[i] = copy;
        if (len_array) len_array[i] = l;
    }
    
    *strs = str_array;
    if (lengths) *lengths = len_array;
    *count = num;
    return KY_CODEC_OK;
}

/* ---------- Query Functions ---------- */

size_t ky_codec_written(const ky_codec_ctx_t *ctx) {
    return ctx ? ctx->pos : 0;
}

size_t ky_codec_remaining(const ky_codec_ctx_t *ctx) {
    if (!ctx) return 0;
    return ctx->len - ctx->pos;
}
