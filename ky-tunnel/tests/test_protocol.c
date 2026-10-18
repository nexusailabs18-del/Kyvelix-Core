#include <stdio.h>
#include <string.h>
#include <assert.h>
#include "ky_protocol.h"

int main() {
    printf("=== Test Protocol ===\n");
    
    /* Test magic check */
    assert(ky_protocol_check_magic("KY01", 4) == 1);
    assert(ky_protocol_check_magic("BAD", 3) == 0);
    printf("✅ Magic check passed\n");
    
    /* Test version validation */
    assert(ky_protocol_is_valid_version(1) == 1);
    assert(ky_protocol_is_valid_version(2) == 0);
    printf("✅ Version validation passed\n");
    
    /* Test header initialization */
    ky_request_header_t req;
    assert(ky_protocol_init_request_header(&req) == 0);
    assert(memcmp(req.magic, "KY01", 4) == 0);
    assert(req.version == 1);
    printf("✅ Request header init passed\n");
    
    ky_response_header_t resp;
    assert(ky_protocol_init_response_header(&resp) == 0);
    assert(memcmp(resp.magic, "KY01", 4) == 0);
    assert(resp.status == KY_STATUS_OK);
    printf("✅ Response header init passed\n");
    
    /* Test command names */
    assert(strcmp(ky_protocol_cmd_name(KY_CMD_FETCH), "FETCH") == 0);
    assert(strcmp(ky_protocol_cmd_name(KY_CMD_PING), "PING") == 0);
    printf("✅ Command names passed\n");
    
    printf("=== Protocol Tests Passed ===\n");
    return 0;
}
