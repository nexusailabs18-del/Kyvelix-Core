#include <stdio.h>
#include <string.h>
#include <assert.h>
#include <stdlib.h>
#include "ky_frame.h"

int main() {
    printf("=== Test Frame ===\n");
    
    /* Test request frame build/parse */
    ky_request_header_t req;
    ky_protocol_init_request_header(&req);
    req.cmd = KY_CMD_FETCH;
    req.flags = KY_FLAG_TLS_SPOOF;
    req.session_id = 42;
    req.url_len = 19;
    
    const char *url = "https://example.com";
    char buffer[256];
    size_t out_len;
    
    assert(ky_frame_build_request(&req, url, 19, buffer, &out_len) == 0);
    assert(out_len == KY_REQUEST_HEADER_SIZE + 19);
    printf("✅ Request frame build passed (len=%zu)\n", out_len);
    
    /* Parse it back */
    ky_request_header_t parsed_req;
    const char *parsed_url;
    size_t parsed_url_len;
    
    assert(ky_frame_parse_request(buffer, out_len, &parsed_req, &parsed_url, &parsed_url_len) == 0);
    assert(parsed_req.cmd == KY_CMD_FETCH);
    assert(parsed_req.flags == KY_FLAG_TLS_SPOOF);
    assert(parsed_req.session_id == 42);
    assert(parsed_url_len == 19);
    assert(memcmp(parsed_url, url, 19) == 0);
    printf("✅ Request frame parse passed\n");
    
    /* Test response frame build/parse */
    ky_response_header_t resp;
    ky_protocol_init_response_header(&resp);
    resp.status = KY_STATUS_OK;
    resp.body_len = 11;
    
    const char *body = "Hello World";
    char resp_buffer[256];
    size_t resp_len;
    
    assert(ky_frame_build_response(&resp, body, 11, resp_buffer, &resp_len) == 0);
    assert(resp_len == KY_RESPONSE_HEADER_SIZE + 11);
    printf("✅ Response frame build passed (len=%zu)\n", resp_len);
    
    /* Parse it back */
    ky_response_header_t parsed_resp;
    const char *parsed_body;
    size_t parsed_body_len;
    
    assert(ky_frame_parse_response(resp_buffer, resp_len, &parsed_resp, &parsed_body, &parsed_body_len) == 0);
    assert(parsed_resp.status == KY_STATUS_OK);
    assert(parsed_body_len == 11);
    assert(memcmp(parsed_body, body, 11) == 0);
    printf("✅ Response frame parse passed\n");
    
    printf("=== Frame Tests Passed ===\n");
    return 0;
}
