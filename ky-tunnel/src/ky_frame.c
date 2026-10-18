#include "ky_frame.h"
#include <string.h>
#include <stdlib.h>

/* ---------- Request Frame Build ---------- */

int ky_frame_build_request(const ky_request_header_t *header,
                           const char *url,
                           size_t url_len,
                           char *out_buffer,
                           size_t *out_len) {
    if (!header || !out_buffer || !out_len) {
        return -1;
    }
    
    if (ky_protocol_validate_request_header(header) != 0) {
        return -1;
    }
    
    if (header->url_len != url_len) {
        return -1;
    }
    
    size_t total = KY_REQUEST_HEADER_SIZE + url_len;
    
    char *p = out_buffer;
    
    memcpy(p, KY_MAGIC, KY_MAGIC_LEN);
    p += KY_MAGIC_LEN;
    
    *p++ = header->version;
    *p++ = header->cmd;
    *p++ = header->flags;
    
    uint32_t session = header->session_id;
    *p++ = (session >> 24) & 0xFF;
    *p++ = (session >> 16) & 0xFF;
    *p++ = (session >> 8) & 0xFF;
    *p++ = session & 0xFF;
    
    uint16_t ulen = (uint16_t)url_len;
    *p++ = (ulen >> 8) & 0xFF;
    *p++ = ulen & 0xFF;
    
    if (url_len > 0 && url != NULL) {
        memcpy(p, url, url_len);
        p += url_len;
    }
    
    *out_len = total;
    return 0;
}

/* ---------- Request Frame Parse ---------- */

int ky_frame_parse_request(const char *buffer,
                           size_t len,
                           ky_request_header_t *header,
                           const char **url,
                           size_t *url_len) {
    if (!buffer || !header || !url || !url_len) {
        return -1;
    }
    
    if (len < KY_REQUEST_HEADER_SIZE) {
        return -1;
    }
    
    if (!ky_protocol_check_magic(buffer, len)) {
        return -1;
    }
    
    const char *p = buffer;
    
    p += KY_MAGIC_LEN;
    
    header->version = (uint8_t)*p++;
    header->cmd = (uint8_t)*p++;
    header->flags = (uint8_t)*p++;
    
    header->session_id = ((uint32_t)(uint8_t)p[0] << 24) |
                         ((uint32_t)(uint8_t)p[1] << 16) |
                         ((uint32_t)(uint8_t)p[2] << 8)  |
                         ((uint32_t)(uint8_t)p[3]);
    p += 4;
    
    header->url_len = ((uint16_t)(uint8_t)p[0] << 8) |
                      ((uint16_t)(uint8_t)p[1]);
    p += 2;
    
    if (len < (size_t)KY_REQUEST_HEADER_SIZE + header->url_len) {
        return -1;
    }
    
    *url = p;
    *url_len = header->url_len;
    
    return 0;
}

/* ---------- Response Frame Build ---------- */

int ky_frame_build_response(const ky_response_header_t *header,
                            const char *body,
                            size_t body_len,
                            char *out_buffer,
                            size_t *out_len) {
    if (!header || !out_buffer || !out_len) {
        return -1;
    }
    
    if (ky_protocol_validate_response_header(header) != 0) {
        return -1;
    }
    
    if (header->body_len != body_len) {
        return -1;
    }
    
    size_t total = KY_RESPONSE_HEADER_SIZE + body_len;
    
    char *p = out_buffer;
    
    memcpy(p, KY_MAGIC, KY_MAGIC_LEN);
    p += KY_MAGIC_LEN;
    
    *p++ = header->status;
    *p++ = header->flags;
    
    uint32_t blen = header->body_len;
    *p++ = (blen >> 24) & 0xFF;
    *p++ = (blen >> 16) & 0xFF;
    *p++ = (blen >> 8) & 0xFF;
    *p++ = blen & 0xFF;
    
    if (body_len > 0 && body != NULL) {
        memcpy(p, body, body_len);
        p += body_len;
    }
    
    *out_len = total;
    return 0;
}

/* ---------- Response Frame Parse ---------- */

int ky_frame_parse_response(const char *buffer,
                            size_t len,
                            ky_response_header_t *header,
                            const char **body,
                            size_t *body_len) {
    if (!buffer || !header || !body || !body_len) {
        return -1;
    }
    
    if (len < KY_RESPONSE_HEADER_SIZE) {
        return -1;
    }
    
    if (!ky_protocol_check_magic(buffer, len)) {
        return -1;
    }
    
    const char *p = buffer;
    
    p += KY_MAGIC_LEN;
    
    header->status = (uint8_t)*p++;
    header->flags = (uint8_t)*p++;
    
    header->body_len = ((uint32_t)(uint8_t)p[0] << 24) |
                       ((uint32_t)(uint8_t)p[1] << 16) |
                       ((uint32_t)(uint8_t)p[2] << 8)  |
                       ((uint32_t)(uint8_t)p[3]);
    p += 4;
    
    if (len < (size_t)KY_RESPONSE_HEADER_SIZE + header->body_len) {
        return -1;
    }
    
    *body = p;
    *body_len = header->body_len;
    
    return 0;
}

/* ---------- Magic Check ---------- */

int ky_frame_check_magic(const char *buffer, size_t len) {
    return ky_protocol_check_magic(buffer, len);
}
