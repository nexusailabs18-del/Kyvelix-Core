#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <sys/time.h>
#include "ky_protocol.h"
#include "ky_frame.h"
#include "ky_codec.h"
#include "ky_tunnel.h"

void bench_frame_operations(int iterations) {
    printf("\n=== Frame Operations Benchmark ===\n");
    printf("Iterations: %d\n\n", iterations);
    
    ky_request_header_t req;
    ky_protocol_init_request_header(&req);
    req.cmd = KY_CMD_FETCH;
    req.session_id = 42;
    req.url_len = 19;
    
    const char *url = "https://example.com";
    char buffer[256];
    size_t out_len;
    
    struct timeval start, end;
    
    gettimeofday(&start, NULL);
    for (int i = 0; i < iterations; i++) {
        ky_frame_build_request(&req, url, 19, buffer, &out_len);
    }
    gettimeofday(&end, NULL);
    
    long build_us = (end.tv_sec - start.tv_sec) * 1000000 + (end.tv_usec - start.tv_usec);
    double build_ns = (double)build_us * 1000.0 / iterations;
    printf("Frame Build: %.2f ns/op\n", build_ns);
    
    gettimeofday(&start, NULL);
    for (int i = 0; i < iterations; i++) {
        ky_request_header_t parsed;
        const char *parsed_url;
        size_t parsed_len;
        ky_frame_parse_request(buffer, out_len, &parsed, &parsed_url, &parsed_len);
    }
    gettimeofday(&end, NULL);
    
    long parse_us = (end.tv_sec - start.tv_sec) * 1000000 + (end.tv_usec - start.tv_usec);
    double parse_ns = (double)parse_us * 1000.0 / iterations;
    printf("Frame Parse: %.2f ns/op\n", parse_ns);
    
    uint8_t codec_buffer[256];
    ky_codec_ctx_t writer;
    
    gettimeofday(&start, NULL);
    for (int i = 0; i < iterations; i++) {
        ky_codec_init_writer(&writer, codec_buffer, sizeof(codec_buffer));
        ky_codec_write_u8(&writer, 42);
        ky_codec_write_u16(&writer, 12345);
        ky_codec_write_u32(&writer, 987654321);
        ky_codec_write_string(&writer, "hello world");
    }
    gettimeofday(&end, NULL);
    
    long codec_us = (end.tv_sec - start.tv_sec) * 1000000 + (end.tv_usec - start.tv_usec);
    double codec_ns = (double)codec_us * 1000.0 / iterations;
    printf("Codec Write: %.2f ns/op\n", codec_ns);
}

void bench_protocol_overhead() {
    printf("\n=== Protocol Overhead Comparison ===\n\n");
    
    const char *http_request = 
        "POST /fetch HTTP/1.1\r\n"
        "Host: kyvelix-node:8080\r\n"
        "Content-Type: application/json\r\n"
        "User-Agent: kyvelix/1.0\r\n"
        "Accept: */*\r\n"
        "Content-Length: 45\r\n"
        "\r\n"
        "{\"url\":\"https://example.com\",\"tls\":true}";
    
    size_t http_len = strlen(http_request);
    
    ky_request_header_t req;
    ky_protocol_init_request_header(&req);
    req.cmd = KY_CMD_FETCH;
    req.flags = KY_FLAG_TLS_SPOOF;
    req.session_id = 1;
    req.url_len = 19;
    
    char ky_buffer[256];
    size_t ky_len;
    ky_frame_build_request(&req, "https://example.com", 19, ky_buffer, &ky_len);
    
    printf("HTTP Request: %zu bytes\n", http_len);
    printf("KY Request:   %zu bytes\n", ky_len);
    printf("Overhead reduction: %.1f%%\n", (1.0 - (double)ky_len / http_len) * 100.0);
    printf("Space saved: %zu bytes per request\n", http_len - ky_len);
    
    int pages = 10000;
    size_t http_total = http_len * pages;
    size_t ky_total = ky_len * pages;
    
    printf("\nFor %d pages:\n", pages);
    printf("HTTP: %zu KB\n", http_total / 1024);
    printf("KY:   %zu KB\n", ky_total / 1024);
    printf("Bandwidth saved: %zu KB\n", (http_total - ky_total) / 1024);
}

int main() {
    printf("=== KY Tunnel vs HTTP Benchmark ===\n");
    bench_frame_operations(1000000);
    bench_protocol_overhead();
    return 0;
}
