#ifndef KYJIT_DPDK_H
#define KYJIT_DPDK_H

#include <stdint.h>

/*
 * SAFE DPDK API
 * 
 * When DPDK is NOT compiled (KYJIT_HAS_DPDK undefined),
 * all functions use void* instead of struct rte_mbuf*.
 * This allows compilation without DPDK headers.
 */

int kyjit_dpdk_init(int argc, char **argv);
int kyjit_dpdk_port_count(void);
int kyjit_dpdk_rx_burst(int port_id, uint16_t queue_id,
                        void **bufs, int max_packets);
void* kyjit_dpdk_packet_data(void *mbuf);
int kyjit_dpdk_packet_len(void *mbuf);
void kyjit_dpdk_free_burst(void **bufs, int nb_pkts);
void kyjit_dpdk_cleanup(void);

#endif
