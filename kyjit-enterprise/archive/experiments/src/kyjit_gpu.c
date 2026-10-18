#define CL_TARGET_OPENCL_VERSION 300
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include "kyjit_gpu.h"

#if defined(KYJIT_HAS_OPENCL)
#include <CL/cl.h>

static cl_context context = NULL;
static cl_command_queue queue = NULL;
static cl_program program = NULL;
static cl_kernel kernel_mfr = NULL;
static cl_device_id device;
static int gpu_ready = 0;
static int is_dedicated = 0;

static const char *mfr_kernel_src = 
    "__kernel void mfr_kernel(__global const int *data, __global int *result, int n) {\n"
    "    int gid = get_global_id(0);\n"
    "    if (gid >= n) return;\n"
    "    int v = data[gid] * 2;\n"
    "    if (v % 3 == 0) {\n"
    "        atomic_add((volatile __global int*)result, v);\n"
    "    }\n"
    "}\n";

int kyjit_gpu_init(void) {
    if (gpu_ready) return 0;
    
    cl_platform_id platform;
    cl_int err;
    char device_name[128];
    cl_device_type dev_type;
    
    err = clGetPlatformIDs(1, &platform, NULL);
    if (err != CL_SUCCESS) return -1;
    
    err = clGetDeviceIDs(platform, CL_DEVICE_TYPE_GPU, 1, &device, NULL);
    if (err != CL_SUCCESS) return -1;
    
    /* Check if DEDICATED (NVIDIA/AMD) or iGPU (Intel) */
    clGetDeviceInfo(device, CL_DEVICE_NAME, sizeof(device_name), device_name, NULL);
    clGetDeviceInfo(device, CL_DEVICE_TYPE, sizeof(dev_type), &dev_type, NULL);
    
    if (strstr(device_name, "Intel") || strstr(device_name, "Iris")) {
        printf("⚠️ Intel iGPU detected (%s) — slower than CPU, skipping GPU\n", device_name);
        return -1;
    }
    is_dedicated = 1;
    
    context = clCreateContext(NULL, 1, &device, NULL, NULL, &err);
    queue = clCreateCommandQueueWithProperties(context, device, NULL, &err);
    
    program = clCreateProgramWithSource(context, 1, &mfr_kernel_src, NULL, &err);
    clBuildProgram(program, 1, &device, NULL, NULL, NULL);
    kernel_mfr = clCreateKernel(program, "mfr_kernel", &err);
    
    gpu_ready = 1;
    printf("✅ Dedicated GPU ready: %s\n", device_name);
    return 0;
}

int kyjit_gpu_available(void) {
    return gpu_ready && is_dedicated;
}

int kyjit_gpu_mfr(const int32_t *data, int n, long long *result) {
    if (!gpu_ready || !is_dedicated) return -1;
    
    cl_int err;
    int zero = 0;
    
    cl_mem data_buf = clCreateBuffer(context, CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR,
                                      n * sizeof(int32_t), (void*)data, &err);
    cl_mem result_buf = clCreateBuffer(context, CL_MEM_READ_WRITE | CL_MEM_COPY_HOST_PTR,
                                        sizeof(int), &zero, &err);
    
    clSetKernelArg(kernel_mfr, 0, sizeof(cl_mem), &data_buf);
    clSetKernelArg(kernel_mfr, 1, sizeof(cl_mem), &result_buf);
    clSetKernelArg(kernel_mfr, 2, sizeof(int), &n);
    
    size_t global_size = ((n + 255) / 256) * 256;
    clEnqueueNDRangeKernel(queue, kernel_mfr, 1, NULL, &global_size, NULL, 0, NULL, NULL);
    
    int gpu_result;
    clEnqueueReadBuffer(queue, result_buf, CL_TRUE, 0, sizeof(int), &gpu_result, 0, NULL, NULL);
    *result = gpu_result;
    
    clReleaseMemObject(data_buf);
    clReleaseMemObject(result_buf);
    return 0;
}

void kyjit_gpu_cleanup(void) {
    if (kernel_mfr) clReleaseKernel(kernel_mfr);
    if (program) clReleaseProgram(program);
    if (queue) clReleaseCommandQueue(queue);
    if (context) clReleaseContext(context);
    gpu_ready = 0;
    is_dedicated = 0;
}

#else
int kyjit_gpu_init(void) { return -1; }
int kyjit_gpu_available(void) { return 0; }
int kyjit_gpu_mfr(const int32_t *data, int n, long long *result) {
    (void)data; (void)n; (void)result;
    return -1;
}
void kyjit_gpu_cleanup(void) {}
#endif
