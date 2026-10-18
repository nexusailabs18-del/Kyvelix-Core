#define _POSIX_C_SOURCE 200112L
#define _DEFAULT_SOURCE

#include "ky_protocol.h"
#include <string.h>
#include <stdlib.h>
#include <sys/types.h>  /* for ssize_t */

/* ---------- Magic and Version Helpers ---------- */

int ky_protocol_check_magic(const char *buffer, size_t len) {
    if (len < KY_MAGIC_LEN) {
        return 0;
    }
    return (memcmp(buffer, KY_MAGIC, KY_MAGIC_LEN) == 0);
}

int ky_protocol_is_valid_version(uint8_t version) {
    return (version == KY_PROTOCOL_VERSION);
}

/* ---------- Command and Status Name Helpers ---------- */

const char *ky_protocol_cmd_name(uint8_t cmd) {
    switch (cmd) {
        case KY_CMD_FETCH: return "FETCH";
        case KY_CMD_BATCH: return "BATCH";
        case KY_CMD_CRAWL: return "CRAWL";
        case KY_CMD_PING:  return "PING";
        default:           return "UNKNOWN";
    }
}

const char *ky_protocol_status_name(uint8_t status) {
    switch (status) {
        case KY_STATUS_OK:      return "OK";
        case KY_STATUS_ERROR:   return "ERROR";
        case KY_STATUS_TIMEOUT: return "TIMEOUT";
        case KY_STATUS_NOMEM:   return "NOMEM";
        case KY_STATUS_INVALID: return "INVALID";
        default:                return "UNKNOWN";
    }
}

/* ---------- Initialization Helpers ---------- */

int ky_protocol_init_request_header(ky_request_header_t *header) {
    if (!header) return -1;
    memset(header, 0, sizeof(*header));
    memcpy(header->magic, KY_MAGIC, KY_MAGIC_LEN);
    header->version = KY_PROTOCOL_VERSION;
    header->cmd = 0;
    header->flags = 0;
    header->session_id = 0;
    header->url_len = 0;
    return 0;
}

int ky_protocol_init_response_header(ky_response_header_t *header) {
    if (!header) return -1;
    memset(header, 0, sizeof(*header));
    memcpy(header->magic, KY_MAGIC, KY_MAGIC_LEN);
    header->status = KY_STATUS_OK;
    header->flags = 0;
    header->body_len = 0;
    return 0;
}

/* ---------- Validation Helpers ---------- */

int ky_protocol_validate_request_header(const ky_request_header_t *header) {
    if (!header) return -1;
    if (!ky_protocol_check_magic(header->magic, KY_MAGIC_LEN)) return -1;
    if (!ky_protocol_is_valid_version(header->version)) return -1;
    
    if ((header->cmd == KY_CMD_FETCH || header->cmd == KY_CMD_CRAWL) && header->url_len == 0) {
        return -1;
    }
    if (header->cmd == KY_CMD_PING && header->url_len != 0) {
        return -1;
    }
    return 0;
}

int ky_protocol_validate_response_header(const ky_response_header_t *header) {
    if (!header) return -1;
    if (!ky_protocol_check_magic(header->magic, KY_MAGIC_LEN)) return -1;
    if (header->status > KY_STATUS_INVALID) return -1;
    return 0;
}

/* ---------- Utility: Compute total frame length ---------- */

ssize_t ky_protocol_request_frame_size(const ky_request_header_t *header) {
    if (!header) return -1;
    return (ssize_t)KY_REQUEST_HEADER_SIZE + header->url_len;
}

ssize_t ky_protocol_response_frame_size(const ky_response_header_t *header) {
    if (!header) return -1;
    return (ssize_t)KY_RESPONSE_HEADER_SIZE + header->body_len;
}
