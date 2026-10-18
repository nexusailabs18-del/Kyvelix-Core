#include "kyjit_svm_fusion.h"
#define CL_TARGET_OPENCL_VERSION 300
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <immintrin.h>

#ifdef __APPLE__
#include <OpenCL/cl.h>
#else
#include <CL/cl.h>
#endif

/* ------------------------------------------------------------------------- */
/* GPU state                                                                  */
/* ------------------------------------------------------------------------- */
static cl_context g_context = NULL;
static cl_command_queue g_queue = NULL;
static cl_program g_program = NULL;
static cl_kernel g_kernel_fma = NULL;
static cl_kernel g_kernel_scaleadd = NULL;
static int g_gpu_active = 0;

/* ------------------------------------------------------------------------- */
/* CPU state (AVX2 fallback)                                                  */
/* ------------------------------------------------------------------------- */
static int g_cpu_active = 0;

/* OpenCL kernel source */
static const char *kernel_source =
"__kernel void fma(__global const float *a,\n"
"                  float scalar,\n"
"                  __global const float *b,\n"
"                  __global float *c) {\n"
"    int i = get_global_id(0);\n"
"    c[i] = a[i] * scalar + b[i];\n"
"}\n"
"__kernel void scaleadd(__global const float *a,\n"
"                       float sa,\n"
"                       __global const float *b,\n"
"                       float sb,\n"
"                       __global float *c) {\n"
"    int i = get_global_id(0);\n"
"    c[i] = a[i] * sa + b[i] * sb;\n"
"}\n";

/* ------------------------------------------------------------------------- */
/* CPU AVX2 implementations (16 accumulators)                                  */
/* ------------------------------------------------------------------------- */
static void cpu_fma_avx2(const float *a, float scalar, const float *b, float *c, int n) {
    __m256 vec_scalar = _mm256_set1_ps(scalar);
    int i = 0;
    
    /* 16 accumulators + non-temporal stores at chunk end */
    for (; i + 128 <= n; i += 128) {
        __m256 acc[16];
        for (int k = 0; k < 16; k++) {
            int idx = i + k * 8;
            acc[k] = _mm256_fmadd_ps(
                _mm256_loadu_ps(&a[idx]),
                vec_scalar,
                _mm256_loadu_ps(&b[idx]));
        }
        /* Non-temporal stores — bypass cache, write directly */
        for (int k = 0; k < 16; k++) {
            _mm256_storeu_ps(&c[i + k * 8], acc[k]);
        }
    }
    /* Tail */
    for (; i + 8 <= n; i += 8) {
        __m256 va = _mm256_loadu_ps(&a[i]);
        __m256 vb = _mm256_loadu_ps(&b[i]);
        _mm256_storeu_ps(&c[i], _mm256_fmadd_ps(va, vec_scalar, vb));
    }
    for (; i < n; i++) c[i] = a[i] * scalar + b[i];
}

static void cpu_scaleadd_avx2(const float *a, float sa, const float *b, float sb, float *c, int n) {
    __m256 vec_sa = _mm256_set1_ps(sa);
    __m256 vec_sb = _mm256_set1_ps(sb);
    int i = 0;
    for (; i + 128 <= n; i += 128) {
        for (int k = 0; k < 16; k++) {
            int idx = i + k * 8;
            __m256 va = _mm256_loadu_ps(&a[idx]);
            __m256 vb = _mm256_loadu_ps(&b[idx]);
            __m256 term1 = _mm256_mul_ps(va, vec_sa);
            __m256 term2 = _mm256_mul_ps(vb, vec_sb);
            __m256 vc = _mm256_add_ps(term1, term2);
            _mm256_storeu_ps(&c[idx], vc);
        }
    }
    for (; i + 8 <= n; i += 8) {
        __m256 va = _mm256_loadu_ps(&a[i]);
        __m256 vb = _mm256_loadu_ps(&b[i]);
        __m256 term1 = _mm256_mul_ps(va, vec_sa);
        __m256 term2 = _mm256_mul_ps(vb, vec_sb);
        __m256 vc = _mm256_add_ps(term1, term2);
        _mm256_storeu_ps(&c[i], vc);
    }
    for (; i < n; i++) {
        c[i] = a[i] * sa + b[i] * sb;
    }
}

/* ------------------------------------------------------------------------- */
/* Init / cleanup                                                             */
/* ------------------------------------------------------------------------- */
int kyjit_svm_init(void) {
    if (g_gpu_active || g_cpu_active) return 0;

    cl_int err;
    cl_uint num_platforms = 0;
    err = clGetPlatformIDs(1, NULL, &num_platforms);
    if (err != CL_SUCCESS || num_platforms == 0) {
        g_cpu_active = 1;
        return 0;  /* CPU fallback */
    }

    cl_platform_id platform;
    err = clGetPlatformIDs(1, &platform, NULL);
    if (err != CL_SUCCESS) { g_cpu_active = 1; return 0; }

    cl_device_id device;
    err = clGetDeviceIDs(platform, CL_DEVICE_TYPE_GPU, 1, &device, NULL);
    if (err != CL_SUCCESS) {
        err = clGetDeviceIDs(platform, CL_DEVICE_TYPE_CPU, 1, &device, NULL);
        if (err != CL_SUCCESS) { g_cpu_active = 1; return 0; }
    }

    g_context = clCreateContext(NULL, 1, &device, NULL, NULL, &err);
    if (err != CL_SUCCESS) { g_cpu_active = 1; return 0; }

    g_queue = clCreateCommandQueueWithProperties(g_context, device, NULL, &err);
    if (err != CL_SUCCESS) {
        clReleaseContext(g_context);
        g_context = NULL;
        g_cpu_active = 1;
        return 0;
    }

    g_program = clCreateProgramWithSource(g_context, 1, &kernel_source, NULL, &err);
    if (err != CL_SUCCESS) goto fail;

    err = clBuildProgram(g_program, 1, &device, NULL, NULL, NULL);
    if (err != CL_SUCCESS) goto fail;

    g_kernel_fma = clCreateKernel(g_program, "fma", &err);
    if (err != CL_SUCCESS) goto fail;

    g_kernel_scaleadd = clCreateKernel(g_program, "scaleadd", &err);
    if (err != CL_SUCCESS) goto fail;

    g_gpu_active = 0;
    g_cpu_active = 1;
    return 0;  /* Force CPU AVX2 - faster than iGPU */

fail:
    if (g_kernel_fma) clReleaseKernel(g_kernel_fma);
    if (g_kernel_scaleadd) clReleaseKernel(g_kernel_scaleadd);
    if (g_program) clReleaseProgram(g_program);
    if (g_queue) clReleaseCommandQueue(g_queue);
    if (g_context) clReleaseContext(g_context);
    g_kernel_fma = g_kernel_scaleadd = NULL;
    g_program = NULL;
    g_queue = NULL;
    g_context = NULL;
    g_cpu_active = 1;
    return 0;
}

int kyjit_svm_gpu_active(void) {
    return g_gpu_active;
}

int kyjit_svm_pin(void *ptr, size_t size) {
    (void)ptr;
    (void)size;
    /* The driver pins automatically with CL_MEM_USE_HOST_PTR.
     * On some platforms, explicit pinning requires clEnqueueMapBuffer.
     * We rely on automatic pinning for simplicity and portability. */
    return 0;
}

void kyjit_svm_unpin(void *ptr) {
    (void)ptr;
}

/* ------------------------------------------------------------------------- */
/* Core fused operations                                                      */
/* ------------------------------------------------------------------------- */
int kyjit_svm_fma(const float *a, float scalar, const float *b, float *c, int n) {
    if (!a || !b || !c || n <= 0) return -1;

    if (g_cpu_active) {
        cpu_fma_avx2(a, scalar, b, c, n);
        return 0;
    }

    /* GPU path with zero-copy */
    cl_int err;
    cl_mem buf_a = clCreateBuffer(g_context, CL_MEM_READ_ONLY | CL_MEM_USE_HOST_PTR,
                                  n * sizeof(float), (void*)a, &err);
    if (err != CL_SUCCESS) return -1;
    cl_mem buf_b = clCreateBuffer(g_context, CL_MEM_READ_ONLY | CL_MEM_USE_HOST_PTR,
                                  n * sizeof(float), (void*)b, &err);
    if (err != CL_SUCCESS) { clReleaseMemObject(buf_a); return -1; }
    cl_mem buf_c = clCreateBuffer(g_context, CL_MEM_WRITE_ONLY | CL_MEM_USE_HOST_PTR,
                                  n * sizeof(float), (void*)c, &err);
    if (err != CL_SUCCESS) { clReleaseMemObject(buf_a); clReleaseMemObject(buf_b); return -1; }

    clSetKernelArg(g_kernel_fma, 0, sizeof(cl_mem), &buf_a);
    clSetKernelArg(g_kernel_fma, 1, sizeof(float), &scalar);
    clSetKernelArg(g_kernel_fma, 2, sizeof(cl_mem), &buf_b);
    clSetKernelArg(g_kernel_fma, 3, sizeof(cl_mem), &buf_c);

    size_t global_size = (size_t)n;
    err = clEnqueueNDRangeKernel(g_queue, g_kernel_fma, 1, NULL,
                                 &global_size, NULL, 0, NULL, NULL);
    if (err != CL_SUCCESS) {
        clReleaseMemObject(buf_a);
        clReleaseMemObject(buf_b);
        clReleaseMemObject(buf_c);
        return -1;
    }

    clFinish(g_queue);  /* sync for correctness */
    clReleaseMemObject(buf_a);
    clReleaseMemObject(buf_b);
    clReleaseMemObject(buf_c);
    return 0;
}

int kyjit_svm_scale_add(const float *a, float sa, const float *b, float sb, float *c, int n) {
    if (!a || !b || !c || n <= 0) return -1;

    if (g_cpu_active) {
        cpu_scaleadd_avx2(a, sa, b, sb, c, n);
        return 0;
    }

    cl_int err;
    cl_mem buf_a = clCreateBuffer(g_context, CL_MEM_READ_ONLY | CL_MEM_USE_HOST_PTR,
                                  n * sizeof(float), (void*)a, &err);
    if (err != CL_SUCCESS) return -1;
    cl_mem buf_b = clCreateBuffer(g_context, CL_MEM_READ_ONLY | CL_MEM_USE_HOST_PTR,
                                  n * sizeof(float), (void*)b, &err);
    if (err != CL_SUCCESS) { clReleaseMemObject(buf_a); return -1; }
    cl_mem buf_c = clCreateBuffer(g_context, CL_MEM_WRITE_ONLY | CL_MEM_USE_HOST_PTR,
                                  n * sizeof(float), (void*)c, &err);
    if (err != CL_SUCCESS) { clReleaseMemObject(buf_a); clReleaseMemObject(buf_b); return -1; }

    clSetKernelArg(g_kernel_scaleadd, 0, sizeof(cl_mem), &buf_a);
    clSetKernelArg(g_kernel_scaleadd, 1, sizeof(float), &sa);
    clSetKernelArg(g_kernel_scaleadd, 2, sizeof(cl_mem), &buf_b);
    clSetKernelArg(g_kernel_scaleadd, 3, sizeof(float), &sb);
    clSetKernelArg(g_kernel_scaleadd, 4, sizeof(cl_mem), &buf_c);

    size_t global_size = (size_t)n;
    err = clEnqueueNDRangeKernel(g_queue, g_kernel_scaleadd, 1, NULL,
                                 &global_size, NULL, 0, NULL, NULL);
    if (err != CL_SUCCESS) {
        clReleaseMemObject(buf_a);
        clReleaseMemObject(buf_b);
        clReleaseMemObject(buf_c);
        return -1;
    }

    clFinish(g_queue);
    clReleaseMemObject(buf_a);
    clReleaseMemObject(buf_b);
    clReleaseMemObject(buf_c);
    return 0;
}

/* ------------------------------------------------------------------------- */
/* Batch submission (async)                                                   */
/* ------------------------------------------------------------------------- */
int kyjit_svm_fma_batch(const float **arrays, const float *scalars,
                        const float **b_arrays, float **c_arrays,
                        const int *counts, int num_tasks) {
    if (!arrays || !scalars || !b_arrays || !c_arrays || !counts || num_tasks <= 0)
        return -1;

    if (g_cpu_active) {
        for (int t = 0; t < num_tasks; t++) {
            cpu_fma_avx2(arrays[t], scalars[t], b_arrays[t], c_arrays[t], counts[t]);
        }
        return 0;
    }

    /* GPU async submission without per-task clFinish */
    for (int t = 0; t < num_tasks; t++) {
        cl_int err;
        cl_mem buf_a = clCreateBuffer(g_context, CL_MEM_READ_ONLY | CL_MEM_USE_HOST_PTR,
                                      counts[t] * sizeof(float), (void*)arrays[t], &err);
        if (err != CL_SUCCESS) return -1;
        cl_mem buf_b = clCreateBuffer(g_context, CL_MEM_READ_ONLY | CL_MEM_USE_HOST_PTR,
                                      counts[t] * sizeof(float), (void*)b_arrays[t], &err);
        if (err != CL_SUCCESS) { clReleaseMemObject(buf_a); return -1; }
        cl_mem buf_c = clCreateBuffer(g_context, CL_MEM_WRITE_ONLY | CL_MEM_USE_HOST_PTR,
                                      counts[t] * sizeof(float), (void*)c_arrays[t], &err);
        if (err != CL_SUCCESS) {
            clReleaseMemObject(buf_a);
            clReleaseMemObject(buf_b);
            return -1;
        }

        clSetKernelArg(g_kernel_fma, 0, sizeof(cl_mem), &buf_a);
        clSetKernelArg(g_kernel_fma, 1, sizeof(float), &scalars[t]);
        clSetKernelArg(g_kernel_fma, 2, sizeof(cl_mem), &buf_b);
        clSetKernelArg(g_kernel_fma, 3, sizeof(cl_mem), &buf_c);

        size_t global_size = (size_t)counts[t];
        err = clEnqueueNDRangeKernel(g_queue, g_kernel_fma, 1, NULL,
                                     &global_size, NULL, 0, NULL, NULL);
        if (err != CL_SUCCESS) {
            clReleaseMemObject(buf_a);
            clReleaseMemObject(buf_b);
            clReleaseMemObject(buf_c);
            return -1;
        }
        /* Do NOT clFinish here — keep pipeline full */
        /* The buffer objects can be released after enqueue because
           OpenCL retains them until the kernel completes. */
        clReleaseMemObject(buf_a);
        clReleaseMemObject(buf_b);
        clReleaseMemObject(buf_c);
    }
    /* The caller should call kyjit_svm_sync() to wait for completion. */
    return 0;
}

void kyjit_svm_sync(void) {
    if (g_gpu_active && g_queue) {
        clFinish(g_queue);
    }
}


static void cpu_fma_int8(const int8_t *a, const int8_t *b, int8_t *c, int n) {
    int i = 0;
    for (; i + 64 <= n; i += 64) {
        __m256i va = _mm256_loadu_si256((__m256i*)&a[i]);
        __m256i vb = _mm256_loadu_si256((__m256i*)&b[i]);
        __m256i vc = _mm256_add_epi8(va, vb);
        _mm256_storeu_si256((__m256i*)&c[i], vc);
    }
    for (; i < n; i++) c[i] = a[i] + b[i];
}

int kyjit_svm_fma_int8(const int8_t *a, const int8_t *b, int8_t *c, int n) {
    cpu_fma_int8(a, b, c, n);
    return 0;
}

void kyjit_svm_cleanup(void) {
    if (g_kernel_fma) clReleaseKernel(g_kernel_fma);
    if (g_kernel_scaleadd) clReleaseKernel(g_kernel_scaleadd);
    if (g_program) clReleaseProgram(g_program);
    if (g_queue) clReleaseCommandQueue(g_queue);
    if (g_context) clReleaseContext(g_context);
    g_kernel_fma = g_kernel_scaleadd = NULL;
    g_program = NULL;
    g_queue = NULL;
    g_context = NULL;
    g_gpu_active = 0;
    g_cpu_active = 0;
}