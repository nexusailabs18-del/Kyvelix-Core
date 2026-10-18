#define _GNU_SOURCE
#include "kyjit_core_pin.h"
#include <pthread.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <unistd.h>
#include <unistd.h>

int kyjit_pin_thread(int core_id) {
    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);
    CPU_SET(core_id, &cpuset);
    return pthread_setaffinity_np(pthread_self(), sizeof(cpu_set_t), &cpuset);
}

int kyjit_pin_thread_to_list(const char *core_list) {
    if (!core_list) return -1;
    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);

    char *copy = strdup(core_list);
    char *token = strtok(copy, ",");
    while (token) {
        int core = atoi(token);
        if (core >= 0 && core < CPU_SETSIZE) {
            CPU_SET(core, &cpuset);
        }
        token = strtok(NULL, ",");
    }
    free(copy);

    return pthread_setaffinity_np(pthread_self(), sizeof(cpu_set_t), &cpuset);
}

int kyjit_get_core_count(void) {
    return (int)sysconf(_SC_NPROCESSORS_ONLN);
}
// Bind current thread to Performance cores (P-cores)
// i5-1240P: 4 P-cores (0-7 with HT), 8 E-cores (8-11)
int kyjit_pin_p_cores(void) {
    cpu_set_t set;
    CPU_ZERO(&set);
    // P-cores are typically 0-7 (with Hyper-Threading)
    for(int i=0; i<8; i++) CPU_SET(i, &set);
    return sched_setaffinity(0, sizeof(set), &set);
}

// Bind to Efficiency cores
int kyjit_pin_e_cores(void) {
    cpu_set_t set;
    CPU_ZERO(&set);
    // E-cores are typically 8-11
    for(int i=8; i<12; i++) CPU_SET(i, &set);
    return sched_setaffinity(0, sizeof(set), &set);
}
