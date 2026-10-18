#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "kyjit_cpu_detect.h"
#include "kyjit_gpu_detect.h"

extern int kyjit_blas_matmul_jit(const float*, const float*, float*, int);

int main(int argc, char *argv[]) {
    if (argc == 1 || (argc > 1 && strcmp(argv[1], "info") == 0)) {
        printf("KYJIT Enterprise v1.0\n");
        printf("372 functions | 211 KB library | 159 KB binary\n\n");
        kyjit_hardware_info();
        printf("\nUsage:\n");
        printf("  ./kyjit info           Show hardware\n");
        printf("  ./kyjit matrix [size]  Matrix benchmark\n");
        return 0;
    }
    
    if (strcmp(argv[1], "matrix") == 0) {
        int N = argc > 2 ? atoi(argv[2]) : 512;
        float *A = aligned_alloc(64, N*N*4);
        float *B = aligned_alloc(64, N*N*4);
        float *C = aligned_alloc(64, N*N*4);
        for(int i=0;i<N*N;i++){A[i]=i%100;B[i]=(i+1)%100;}
        kyjit_blas_matmul_jit(A,B,C,N);
        printf("Matrix %dx%d done. C[0]=%.0f\n", N, N, C[0]);
        free(A); free(B); free(C);
        return 0;
    }
    
    printf("Usage: ./kyjit [info|matrix]\n");
    return 0;
}
