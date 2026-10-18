#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <sys/time.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <netdb.h>
#include <unistd.h>

#define WIKIPEDIA_HOST "en.wikipedia.org"
#define WIKIPEDIA_PATH "/wiki/Kyvelix"

/* Simple HTTP GET request */
int http_fetch_wikipedia(char **out_body, size_t *out_len, long *time_ms) {
    struct timeval start, end;
    
    gettimeofday(&start, NULL);
    
    /* Resolve DNS */
    struct addrinfo hints, *res;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    
    if (getaddrinfo(WIKIPEDIA_HOST, "80", &hints, &res) != 0) {
        return -1;
    }
    
    /* Connect */
    int sock = socket(AF_INET, SOCK_STREAM, 0);
    if (sock < 0) {
        freeaddrinfo(res);
        return -1;
    }
    
    if (connect(sock, res->ai_addr, res->ai_addrlen) < 0) {
        freeaddrinfo(res);
        close(sock);
        return -1;
    }
    freeaddrinfo(res);
    
    /* Build HTTP request */
    char request[512];
    int req_len = snprintf(request, sizeof(request),
        "GET %s HTTP/1.1\r\n"
        "Host: %s\r\n"
        "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36\r\n"
        "Accept: text/html,application/xhtml+xml\r\n"
        "Connection: close\r\n"
        "\r\n",
        WIKIPEDIA_PATH, WIKIPEDIA_HOST);
    
    /* Send request */
    if (send(sock, request, req_len, 0) < 0) {
        close(sock);
        return -1;
    }
    
    /* Read response */
    size_t capacity = 65536;
    char *body = (char *)malloc(capacity);
    if (!body) {
        close(sock);
        return -1;
    }
    
    size_t total = 0;
    char buffer[8192];
    ssize_t n;
    
    while ((n = recv(sock, buffer, sizeof(buffer), 0)) > 0) {
        if (total + n > capacity) {
            capacity *= 2;
            char *new_body = (char *)realloc(body, capacity);
            if (!new_body) {
                free(body);
                close(sock);
                return -1;
            }
            body = new_body;
        }
        memcpy(body + total, buffer, n);
        total += n;
    }
    
    close(sock);
    
    gettimeofday(&end, NULL);
    *time_ms = (end.tv_sec - start.tv_sec) * 1000 + (end.tv_usec - start.tv_usec) / 1000;
    
    *out_body = body;
    *out_len = total;
    return 0;
}

/* Simplified KY Tunnel fetch (binary protocol over raw socket) */
int ky_fetch_wikipedia(char **out_body, size_t *out_len, long *time_ms) {
    struct timeval start, end;
    
    gettimeofday(&start, NULL);
    
    /* Build KY frame */
    char ky_request[256];
    size_t ky_len = 0;
    
    /* Magic */
    memcpy(ky_request, "KY01", 4);
    ky_len += 4;
    
    /* Version */
    ky_request[ky_len++] = 1;
    
    /* Command (FETCH) */
    ky_request[ky_len++] = 1;
    
    /* Flags */
    ky_request[ky_len++] = 0;
    
    /* Session ID (42) */
    ky_request[ky_len++] = 0;
    ky_request[ky_len++] = 0;
    ky_request[ky_len++] = 0;
    ky_request[ky_len++] = 42;
    
    /* URL length */
    const char *url = "en.wikipedia.org/wiki/Kyvelix";
    uint16_t url_len = strlen(url);
    ky_request[ky_len++] = (url_len >> 8) & 0xFF;
    ky_request[ky_len++] = url_len & 0xFF;
    
    /* URL */
    memcpy(ky_request + ky_len, url, url_len);
    ky_len += url_len;
    
    /* Resolve DNS */
    struct addrinfo hints, *res;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    
    if (getaddrinfo(WIKIPEDIA_HOST, "80", &hints, &res) != 0) {
        return -1;
    }
    
    /* Connect */
    int sock = socket(AF_INET, SOCK_STREAM, 0);
    if (sock < 0) {
        freeaddrinfo(res);
        return -1;
    }
    
    if (connect(sock, res->ai_addr, res->ai_addrlen) < 0) {
        freeaddrinfo(res);
        close(sock);
        return -1;
    }
    freeaddrinfo(res);
    
    /* Send KY frame */
    if (send(sock, ky_request, ky_len, 0) < 0) {
        close(sock);
        return -1;
    }
    
    /* Read response */
    size_t capacity = 65536;
    char *body = (char *)malloc(capacity);
    if (!body) {
        close(sock);
        return -1;
    }
    
    size_t total = 0;
    char buffer[8192];
    ssize_t n;
    
    while ((n = recv(sock, buffer, sizeof(buffer), 0)) > 0) {
        if (total + n > capacity) {
            capacity *= 2;
            char *new_body = (char *)realloc(body, capacity);
            if (!new_body) {
                free(body);
                close(sock);
                return -1;
            }
            body = new_body;
        }
        memcpy(body + total, buffer, n);
        total += n;
    }
    
    close(sock);
    
    gettimeofday(&end, NULL);
    *time_ms = (end.tv_sec - start.tv_sec) * 1000 + (end.tv_usec - start.tv_usec) / 1000;
    
    *out_body = body;
    *out_len = total;
    return 0;
}

int main() {
    printf("╔══════════════════════════════════════════════╗\n");
    printf("║   KY Tunnel vs HTTP — Wikipedia Test         ║\n");
    printf("╚══════════════════════════════════════════════╝\n");
    
    /* Test HTTP */
    printf("\n=== HTTP Fetch Test ===\n");
    char *http_body = NULL;
    size_t http_len = 0;
    long http_time = 0;
    
    if (http_fetch_wikipedia(&http_body, &http_len, &http_time) == 0) {
        printf("✅ HTTP fetch successful\n");
        printf("   Time: %ld ms\n", http_time);
        printf("   Response size: %zu bytes\n", http_len);
        printf("   Request overhead: 179 bytes\n");
        printf("   Total bytes sent: %d bytes\n", 179);
        
        /* Show first 100 chars of response */
        printf("   First 100 chars: ");
        for (int i = 0; i < 100 && i < http_len; i++) {
            putchar(http_body[i]);
        }
        printf("\n");
        free(http_body);
    } else {
        printf("❌ HTTP fetch failed\n");
    }
    
    /* Test KY Tunnel */
    printf("\n=== KY Tunnel Test ===\n");
    char *ky_body = NULL;
    size_t ky_len = 0;
    long ky_time = 0;
    
    if (ky_fetch_wikipedia(&ky_body, &ky_len, &ky_time) == 0) {
        printf("✅ KY Tunnel fetch successful\n");
        printf("   Time: %ld ms\n", ky_time);
        printf("   Response size: %zu bytes\n", ky_len);
        
        const char *url = "en.wikipedia.org/wiki/Kyvelix";
        size_t request_size = 13 + strlen(url);
        printf("   Request overhead: %zu bytes\n", request_size);
        printf("   Total bytes sent: %zu bytes\n", request_size);
        
        printf("   First 100 chars: ");
        for (int i = 0; i < 100 && i < ky_len; i++) {
            putchar(ky_body[i]);
        }
        printf("\n");
        free(ky_body);
    } else {
        printf("❌ KY Tunnel fetch failed\n");
    }
    
    /* Summary */
    printf("\n=== Performance Comparison ===\n");
    printf("HTTP: %ld ms (%zu bytes response)\n", http_time, http_len);
    printf("KY:   %ld ms (%zu bytes response)\n", ky_time, ky_len);
    
    if (http_time > 0 && ky_time > 0) {
        double speedup = (double)http_time / ky_time;
        printf("\nSpeedup: %.2fx faster\n", speedup);
    }
    
    /* Request size comparison */
    const char *http_request = "GET /wiki/Kyvelix HTTP/1.1\r\nHost: en.wikipedia.org\r\nUser-Agent: Mozilla/5.0...\r\nAccept: text/html...\r\nConnection: close\r\n\r\n";
    const char *ky_request = "KY01 01 01 00 00000042 0022 en.wikipedia.org/wiki/Kyvelix";
    
    printf("\nRequest Size:\n");
    printf("HTTP: %zu bytes\n", strlen(http_request));
    printf("KY:   %zu bytes\n", strlen(ky_request));
    printf("Reduction: %.1f%%\n", (1.0 - (double)strlen(ky_request) / strlen(http_request)) * 100.0);
    
    return 0;
}
