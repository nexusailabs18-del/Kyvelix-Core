#ifndef KYJIT_GRAPHCOLOR_FULL_H
#define KYJIT_GRAPHCOLOR_FULL_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    uint32_t vreg;
    uint32_t degree;
    int32_t color;
} KyGCRegNode;

typedef struct {
    KyGCRegNode *nodes;
    uint32_t num_nodes;
    uint32_t num_regs;
    uint8_t *adj;
} KyGraphColoring;

static inline KyGraphColoring *ky_gc_create(uint32_t num_nodes, uint32_t num_regs) {
    KyGraphColoring *gc = calloc(1, sizeof(*gc));
    if (!gc) return NULL;
    gc->nodes = calloc(num_nodes, sizeof(KyGCRegNode));
    gc->adj = calloc(num_nodes * num_nodes, sizeof(uint8_t));
    if (!gc->nodes || !gc->adj) { free(gc->nodes); free(gc->adj); free(gc); return NULL; }
    gc->num_nodes = num_nodes;
    gc->num_regs = num_regs;
    for (uint32_t i = 0; i < num_nodes; i++) { gc->nodes[i].vreg = i; gc->nodes[i].color = -1; }
    return gc;
}

static inline void ky_gc_add_edge(KyGraphColoring *gc, uint32_t a, uint32_t b) {
    if (!gc || a >= gc->num_nodes || b >= gc->num_nodes || a == b) return;
    if (!gc->adj[a * gc->num_nodes + b]) {
        gc->adj[a * gc->num_nodes + b] = 1;
        gc->adj[b * gc->num_nodes + a] = 1;
        gc->nodes[a].degree++;
        gc->nodes[b].degree++;
    }
}

// Chaitin-style graph coloring: simplify highest degree first
static inline void ky_gc_color(KyGraphColoring *gc) {
    if (!gc) return;
    // Simple greedy: pick uncolored highest-degree node
    for (uint32_t round = 0; round < gc->num_nodes; round++) {
        int32_t best = -1;
        for (uint32_t i = 0; i < gc->num_nodes; i++) {
            if (gc->nodes[i].color == -1) {
                if (best == -1 || gc->nodes[i].degree > gc->nodes[best].degree) best = (int32_t)i;
            }
        }
        if (best == -1) break;
        uint32_t used = 0;
        for (uint32_t j = 0; j < gc->num_nodes; j++) {
            if (gc->adj[best * gc->num_nodes + j] && gc->nodes[j].color >= 0) {
                used |= (1u << gc->nodes[j].color);
            }
        }
        for (uint32_t r = 0; r < gc->num_regs; r++) {
            if (!(used & (1u << r))) { gc->nodes[best].color = (int32_t)r; break; }
        }
    }
}

static inline int32_t ky_gc_get_color(KyGraphColoring *gc, uint32_t vreg) {
    return gc && vreg < gc->num_nodes ? gc->nodes[vreg].color : -1;
}
#endif
