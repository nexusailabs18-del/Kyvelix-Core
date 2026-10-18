#ifndef KY_PROTOCOL_H
#define KY_PROTOCOL_H

#include <stdint.h>
#include <stddef.h>
#include <sys/types.h>  /* for ssize_t */

/* Magic bytes for frame identification */
#define KY_MAGIC "KY01"
#define KY_MAGIC_LEN 4

/* Protocol version */
#define KY_PROTOCOL_VERSION 1

/* Commands */
#define KY_CMD_FETCH 1
#define KY_CMD_BATCH 2
#define KY_CMD_CRAWL 3
#define KY_CMD_PING  4

/* Flags */
#define KY_FLAG_TLS_SPOOF      (1 << 0)
#define KY_FLAG_STREAM         (1 << 1)
#define KY_FLAG_SESSION_REUSE  (1 << 2)

/* Response status codes */
#define KY_STATUS_OK        0
#define KY_STATUS_ERROR     1
#define KY_STATUS_TIMEOUT   2
#define KY_STATUS_NOMEM     3
#define KY_STATUS_INVALID   4

/* Maximum URL length we support (64 KB) */
#define KY_MAX_URL_LEN      65535

/* Maximum body length for a single frame (4 GB) */
#define KY_MAX_BODY_LEN     0xFFFFFFFF

/* Fixed header sizes */
#define KY_REQUEST_HEADER_SIZE 13
#define KY_RESPONSE_HEADER_SIZE 10

/* Request frame layout */
typedef struct __attribute__((packed)) {
    char     magic[KY_MAGIC_LEN];
    uint8_t  version;
    uint8_t  cmd;
    uint8_t  flags;
    uint32_t session_id;
    uint16_t url_len;
} ky_request_header_t;

/* Response frame layout */
typedef struct __attribute__((packed)) {
    char     magic[KY_MAGIC_LEN];
    uint8_t  status;
    uint8_t  flags;
    uint32_t body_len;
} ky_response_header_t;

/* Protocol utility functions */
int ky_protocol_check_magic(const char *buffer, size_t len);
int ky_protocol_is_valid_version(uint8_t version);
const char *ky_protocol_cmd_name(uint8_t cmd);
const char *ky_protocol_status_name(uint8_t status);
int ky_protocol_init_request_header(ky_request_header_t *header);
int ky_protocol_init_response_header(ky_response_header_t *header);
int ky_protocol_validate_request_header(const ky_request_header_t *header);
int ky_protocol_validate_response_header(const ky_response_header_t *header);
ssize_t ky_protocol_request_frame_size(const ky_request_header_t *header);
ssize_t ky_protocol_response_frame_size(const ky_response_header_t *header);

#endif /* KY_PROTOCOL_H */
