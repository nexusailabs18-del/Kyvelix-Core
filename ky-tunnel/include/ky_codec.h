#ifndef KY_CODEC_H
#define KY_CODEC_H

#include <stddef.h>
#include <stdint.h>

/* Error codes for codec operations */
#define KY_CODEC_OK            0
#define KY_CODEC_ERR_OVERFLOW -1
#define KY_CODEC_ERR_INVALID  -2
#define KY_CODEC_ERR_NOMEM    -3

/* Codec context for sequential read/write of binary data */
typedef struct {
    uint8_t *data;       /* Buffer pointer (writer: writable, reader: read-only) */
    size_t   len;        /* Total buffer length (writer: capacity, reader: length) */
    size_t   pos;        /* Current position */
    int      is_reader;  /* Non-zero if reader, zero if writer */
} ky_codec_ctx_t;

/*
 * Initialize a codec context for writing.
 * @param ctx       Context to initialize.
 * @param buffer    Pre-allocated buffer for writing.
 * @param capacity  Size of buffer in bytes.
 */
void ky_codec_init_writer(ky_codec_ctx_t *ctx, uint8_t *buffer, size_t capacity);

/*
 * Initialize a codec context for reading.
 * @param ctx       Context to initialize.
 * @param buffer    Buffer containing encoded data.
 * @param len       Length of data in buffer.
 */
void ky_codec_init_reader(ky_codec_ctx_t *ctx, const uint8_t *buffer, size_t len);

/*
 * Write a single unsigned 8-bit integer.
 * Returns KY_CODEC_OK or error.
 */
int ky_codec_write_u8(ky_codec_ctx_t *ctx, uint8_t value);

/*
 * Write a single unsigned 16-bit integer in network byte order (big-endian).
 */
int ky_codec_write_u16(ky_codec_ctx_t *ctx, uint16_t value);

/*
 * Write a single unsigned 32-bit integer in network byte order.
 */
int ky_codec_write_u32(ky_codec_ctx_t *ctx, uint32_t value);

/*
 * Write raw bytes.
 */
int ky_codec_write_bytes(ky_codec_ctx_t *ctx, const void *data, size_t len);

/*
 * Write a length-prefixed string.
 * Format: uint16 length (network order) followed by string bytes (no null terminator).
 * Length is limited to 65535 bytes.
 */
int ky_codec_write_string(ky_codec_ctx_t *ctx, const char *str);

/*
 * Write an array of length-prefixed strings.
 * Format: uint16 count, then each string is written as length-prefixed.
 */
int ky_codec_write_string_array(ky_codec_ctx_t *ctx, const char **strs, size_t count);

/*
 * Read a single unsigned 8-bit integer.
 * Returns KY_CODEC_OK or error.
 */
int ky_codec_read_u8(ky_codec_ctx_t *ctx, uint8_t *out);

/*
 * Read a single unsigned 16-bit integer (network order).
 */
int ky_codec_read_u16(ky_codec_ctx_t *ctx, uint16_t *out);

/*
 * Read a single unsigned 32-bit integer (network order).
 */
int ky_codec_read_u32(ky_codec_ctx_t *ctx, uint32_t *out);

/*
 * Read raw bytes into a caller-provided buffer.
 */
int ky_codec_read_bytes(ky_codec_ctx_t *ctx, void *buf, size_t len);

/*
 * Read a length-prefixed string (zero-copy: returns pointer into underlying buffer).
 * @param str   On success, set to point to start of string bytes.
 * @param len   On success, set to length of string.
 * The string is NOT null-terminated; caller must handle length.
 */
int ky_codec_read_string(ky_codec_ctx_t *ctx, const char **str, size_t *len);

/*
 * Read an array of strings (zero-copy: returns array of pointers into buffer).
 * @param strs      On success, allocated array of char* (must be freed by caller).
 * @param count     On success, number of strings.
 * @param lengths   On success, allocated array of size_t lengths (must be freed by caller).
 *                  May be NULL if lengths not needed.
 * Returns KY_CODEC_OK or error.
 */
int ky_codec_read_string_array(ky_codec_ctx_t *ctx, char ***strs, size_t **lengths, size_t *count);

/*
 * Return the number of bytes written (for writer) or remaining unread (for reader).
 */
size_t ky_codec_written(const ky_codec_ctx_t *ctx);
size_t ky_codec_remaining(const ky_codec_ctx_t *ctx);

#endif /* KY_CODEC_H */