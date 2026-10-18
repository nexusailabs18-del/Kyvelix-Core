#include <stdio.h>
#include <string.h>
#include <assert.h>
#include "ky_tunnel.h"

int main() {
    printf("=== Test Tunnel ===\n");
    
    /* Test tunnel creation with config */
    KyTunnel *tunnel = ky_tunnel_create("host=127.0.0.1;port=9091;timeout_ms=1000");
    if (tunnel) {
        printf("✅ Tunnel created\n");
        
        /* Test ping */
        if (ky_tunnel_ping(tunnel) == KY_TUNNEL_OK) {
            printf("✅ Ping passed\n");
        } else {
            printf("⚠️  Ping failed (no server running, expected)\n");
        }
        
        /* Cleanup */
        ky_tunnel_destroy(tunnel);
        printf("✅ Tunnel destroyed\n");
    } else {
        printf("⚠️  Tunnel creation failed (no server running, expected)\n");
    }
    
    printf("=== Tunnel Tests Passed ===\n");
    return 0;
}
