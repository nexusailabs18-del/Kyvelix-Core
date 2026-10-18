#ifndef KYJIT_AI_BRIDGE_H
#define KYJIT_AI_BRIDGE_H

#include <stdint.h>

// FFI functions from Rust library
#ifdef __cplusplus
extern "C" {
#endif

// GEMM: C = A * B (FP32)
void ky_ai_gemm_fp32(const float *a, const float *b, float *c, 
                     int m, int k, int n);

// INT8 quantization
void ky_ai_quantize_int8(const float *input, int8_t *output, 
                         float *scale, int n);

#ifdef __cplusplus
}
#endif

#endif
