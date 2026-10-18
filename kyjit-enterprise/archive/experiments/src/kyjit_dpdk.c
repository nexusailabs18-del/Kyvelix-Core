#include <stdint.h>
#include <string.h>
#include "kyjit_dpdk.h"

/* DPDK kernel bypass wrapper — SAFE VERSION
 * 
 * Compiles WITHOUT DPDK (safe stubs).
 * To enable DPDK: compile with -DKYJIT_HAS_DPDK and link DPDK libs.
 * NEVER binds NIC automatically — caller must explicitly configure.
 */

#if defined(KYJIT_HAS_DPDK)
#include <rte_eal.h>
#include <rte_ethdev.h>
#include <rte_mbuf.h>

static int dpdk_initialized = 0;

int kyjit_dpdk_init(int argc, char **argv) {
    if (dpdk_initialized) return 0;
    int ret = rte_eal_init(argc, argv);
    if (ret < 0) return -1;
    dpdk_initialized = 1;
    return 0;
}

int kyjit_dpdk_port_count(void) {
    if (!dpdk_initialized) return 0;
    return rte_eth_dev_count_avail();
}

int kyjit_dpdk_rx_burst(int port_id, uint16_t queue_id,
                        void **bufs, int max_packets) {
    if (!dpdk_initialized) return -1;
    if (max_packets > 128) max_packets = 128;
    return rte_eth_rx_burst(port_id, queue_id, (struct rte_mbuf**)bufs, max_packets);
}

void* kyjit_dpdk_packet_data(void *mbuf) {
    return rte_pktmbuf_mtod((struct rte_mbuf*)mbuf, void*);
}

int kyjit_dpdk_packet_len(void *mbuf) {
    return rte_pktmbuf_pkt_len((struct rte_mbuf*)mbuf);
}

void kyjit_dpdk_free_burst(void **bufs, int nb_pkts) {
    for (int i = 0; i < nb_pkts; i++) {
        rte_pktmbuf_free((struct rte_mbuf*)bufs[i]);
    }
}

void kyjit_dpdk_cleanup(void) {
    if (!dpdk_initialized) return;
    rte_eal_cleanup();
    dpdk_initialized = 0;
}

#else
/* SAFE STUBS — no DPDK required */
int kyjit_dpdk_init(int argc, char **argv) { (void)argc; (void)argv; return -1; }
int kyjit_dpdk_port_count(void) { return 0; }
int kyjit_dpdk_rx_burst(int port_id, uint16_t queue_id, void **bufs, int max_packets) {
    (void)port_id; (void)queue_id; (void)bufs; (void)max_packets;
    return -1;
}
void* kyjit_dpdk_packet_data(void *mbuf) { (void)mbuf; return NULL; }
int kyjit_dpdk_packet_len(void *mbuf) { (void)mbuf; return 0; }
void kyjit_dpdk_free_burst(void **bufs, int nb_pkts) { (void)bufs; (void)nb_pkts; }
void kyjit_dpdk_cleanup(void) {}
#endif
