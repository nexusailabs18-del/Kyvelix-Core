#include <stdio.h>
#include <string.h>
#include <time.h>
#include <sys/time.h>
#include "ky_protocol.h"
#include "ky_frame.h"
#include "ky_codec.h"

/* Simulate HTTP request building (what HTTP libs do internally) */
long bench_http_build(int iterations) {
    struct timeval start, end;
    char buffer[512];
    
    gettimeofday(&start, NULL);
    for (int i = 0; i < iterations; i++) {
        snprintf(buffer, sizeof(buffer),
            "GET /wiki/Main_Page HTTP/1.1\r\n"
            "Host: en.wikipedia.org\r\n"
            "User-Agent: Mozilla/5.0\r\n"
            "Accept: text/html\r\n"
            "Connection: keep-alive\r\n"
            "\r\n");
    }
    gettimeofday(&end, NULL);
    return (end.tv_sec - start.tv_sec) * 1000000 + (end.tv_usec - start.tv_usec);
}

/* Simulate HTTP response parsing */
long bench_http_parse(int iterations) {
    struct timeval start, end;
    const char *response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 5000\r\n\r\n<html>...</html>";
    
    gettimeofday(&start, NULL);
    for (int i = 0; i < iterations; i++) {
        /* Parse status line */
        char status[64];
        sscanf(response, "HTTP/1.1 %s", status);
        
        /* Find body (after \r\n\r\n) */
        const char *body = strstr(response, "\r\n\r\n");
        if (body) body += 4;
        
        /* Parse Content-Length */
        int content_length = 0;
        const char *cl = strstr(response, "Content-Length:");
        if (cl) sscanf(cl, "Content-Length: %d", &content_length);
    }
    gettimeofday(&end, NULL);
    return (end.tv_sec - start.tv_sec) * 1000000 + (end.tv_usec - start.tv_usec);
}

/* KY Tunnel frame build */
long bench_ky_build(int iterations) {
    struct timeval start, end;
    
    ky_request_header_t req;
    ky_protocol_init_request_header(&req);
    req.cmd = KY_CMD_FETCH;
    req.session_id = 1;
    req.url_len = 30;
    
    char buffer[64];
    size_t out_len;
    
    gettimeofday(&start, NULL);
    for (int i = 0; i < iterations; i++) {
        ky_frame_build_request(&req, "en.wikipedia.org/wiki/Main_Page", 30, buffer, &out_len);
    }
    gettimeofday(&end, NULL);
    return (end.tv_sec - start.tv_sec) * 1000000 + (end.tv_usec - start.tv_usec);
}

/* KY Tunnel frame parse */
long bench_ky_parse(int iterations) {
    struct timeval start, end;
    
    ky_request_header_t req;
    ky_protocol_init_request_header(&req);
    req.cmd = KY_CMD_FETCH;
    req.session_id = 1;
    req.url_len = 30;
    
    char buffer[64];
    size_t out_len;
    ky_frame_build_request(&req, "en.wikipedia.org/wiki/Main_Page", 30, buffer, &out_len);
    
    gettimeofday(&start, NULL);
    for (int i = 0; i < iterations; i++) {
        ky_request_header_t parsed;
        const char *url;
        size_t url_len;
        ky_frame_parse_request(buffer, out_len, &parsed, &url, &url_len);
    }
    gettimeofday(&end, NULL);
    return (end.tv_sec - start.tv_sec) * 1000000 + (end.tv_usec - start.tv_usec);
}

int main() {
    int iterations = 1000000;
    
    printf("╔══════════════════════════════════════════════════════╗\n");
    printf("║   KY Tunnel vs HTTP — Pure Protocol Speed           ║\n");
    printf("╚══════════════════════════════════════════════════════╝\n");
    printf("\n%d iterations each\n\n", iterations);
    
    /* Build benchmarks */
    long http_build_us = bench_http_build(iterations);
    long ky_build_us = bench_ky_build(iterations);
    
    printf("=== Request Building ===\n");
    printf("HTTP: %ld μs (%ld ns/op)\n", http_build_us, http_build_us * 1000 / iterations);
    printf("KY:   %ld μs (%ld ns/op)\n", ky_build_us, ky_build_us * 1000 / iterations);
    printf("Speedup: %.1fx\n", (double)http_build_us / ky_build_us);
    
    /* Parse benchmarks */
    long http_parse_us = bench_http_parse(iterations);
    long ky_parse_us = bench_ky_parse(iterations);
    
    printf("\n=== Response Parsing ===\n");
    printf("HTTP: %ld μs (%ld ns/op)\n", http_parse_us, http_parse_us * 1000 / iterations);
    printf("KY:   %ld μs (%ld ns/op)\n", ky_parse_us, ky_parse_us * 1000 / iterations);
    printf("Speedup: %.1fx\n", (double)http_parse_us / ky_parse_us);
    
    /* Combined */
    printf("\n=== Combined (Build + Parse) ===\n");
    long http_total = http_build_us + http_parse_us;
    long ky_total = ky_build_us + ky_parse_us;
    printf("HTTP: %ld μs\n", http_total);
    printf("KY:   %ld μs\n", ky_total);
    printf("Overall Speedup: %.1fx\n", (double)http_total / ky_total);
    
    /* Extrapolate */
    printf("\n=== For 1,000,000 Requests ===\n");
    printf("HTTP: %.2f seconds (protocol processing only)\n", (double)http_total / 1000000);
    printf("KY:   %.2f seconds (protocol processing only)\n", (double)ky_total / 1000000);
    printf("Time saved: %.2f seconds\n", (double)(http_total - ky_total) / 1000000);
    
    return 0;
}
