#ifndef KY_FRAME_H
#define KY_FRAME_H

#include <stddef.h>
#include <stdint.h>
#include "ky_protocol.h"

/* 
 * Build a complete request frame.
 * 
 * @param header      Pointer to header with cmd, flags, session_id, url_len filled.
 * @param url         Pointer to URL bytes.
 * @param url_len     Length of URL.
 * @param out_buffer  Pre-allocated buffer to receive the complete frame.
 *                    Must be at least KY_REQUEST_HEADER_SIZE + url_len bytes.
 * @param out_len     On success, set to total length of frame written.
 * @return 0 on success, negative on error (e.g., buffer too small).
 */
int ky_frame_build_request(const ky_request_header_t *header,
                           const char *url,
                           size_t url_len,
                           char *out_buffer,
                           size_t *out_len);

/*
 * Parse a request frame from a buffer.
 * 
 * @param buffer      Pointer to the complete frame.
 * @param len         Total length of the frame.
 * @param header      On success, filled with header fields.
 * @param url         On success, points to the URL within the buffer (not copied).
 * @param url_len     On success, length of URL.
 * @return 0 on success, negative on error (e.g., bad magic, truncated).
 */
int ky_frame_parse_request(const char *buffer,
                           size_t len,
                           ky_request_header_t *header,
                           const char **url,
                           size_t *url_len);

/*
 * Build a complete response frame.
 * 
 * @param header      Pointer to header with status, flags, body_len filled.
 * @param body        Pointer to body bytes (may be NULL if body_len = 0).
 * @param body_len    Length of body.
 * @param out_buffer  Pre-allocated buffer to receive the complete frame.
 *                    Must be at least KY_RESPONSE_HEADER_SIZE + body_len bytes.
 * @param out_len     On success, set to total length of frame written.
 * @return 0 on success, negative on error.
 */
int ky_frame_build_response(const ky_response_header_t *header,
                            const char *body,
                            size_t body_len,
                            char *out_buffer,
                            size_t *out_len);

/*
 * Parse a response frame from a buffer.
 * 
 * @param buffer      Pointer to the complete frame.
 * @param len         Total length of the frame.
 * @param header      On success, filled with header fields.
 * @param body        On success, points to the body within the buffer (not copied).
 * @param body_len    On success, length of body.
 * @return 0 on success, negative on error.
 */
int ky_frame_parse_response(const char *buffer,
                            size_t len,
                            ky_response_header_t *header,
                            const char **body,
                            size_t *body_len);

/*
 * Validate that a buffer starts with the KY magic bytes.
 * Returns 1 if valid, 0 otherwise.
 */
int ky_frame_check_magic(const char *buffer, size_t len);

#endif /* KY_FRAME_H */