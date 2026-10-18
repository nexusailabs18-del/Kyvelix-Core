#ifndef KYJIT_CPU_DETECT_H
#define KYJIT_CPU_DETECT_H

int kyjit_cpu_has_avx512(void);
int kyjit_cpu_has_avx2(void);
int kyjit_cpu_has_fma(void);
int kyjit_cpu_has_sse4(void);
int kyjit_cpu_count(void);
const char* kyjit_cpu_name(void);
const char* kyjit_cpu_best_path(void);
void kyjit_cpu_info(void);

#endif
