#include "kyjit_cha.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#define MAX_METHODS_PER_CLASS 64

struct KyjitCHA {
    KyjitClass *classes;
    int         max_classes;
    int         class_count;
    int        *method_to_class;   /* method_id → class_id (last/only) */
    int        *method_count;      /* method_id → count of implementations */
    int         max_methods;
};

KyjitCHA *kyjit_cha_create(int max_classes) {
    KyjitCHA *cha = calloc(1, sizeof(*cha));
    cha->max_classes = max_classes > 0 ? max_classes : 256;
    cha->classes = calloc(cha->max_classes, sizeof(KyjitClass));
    cha->method_to_class = calloc(cha->max_classes * MAX_METHODS_PER_CLASS, sizeof(int));
    cha->method_count   = calloc(cha->max_classes * MAX_METHODS_PER_CLASS, sizeof(int));
    cha->max_methods = cha->max_classes * MAX_METHODS_PER_CLASS;
    return cha;
}

int kyjit_cha_add_class(KyjitCHA *cha, int class_id, int parent_id, int is_abstract) {
    if (!cha || class_id >= cha->max_classes) return -1;
    if (class_id >= cha->class_count) cha->class_count = class_id + 1;

    KyjitClass *c = &cha->classes[class_id];
    c->class_id = class_id;
    c->parent_id = parent_id;
    c->is_abstract = is_abstract;
    c->method_count = 0;
    c->method_ids = NULL;
    return 0;
}

int kyjit_cha_add_method(KyjitCHA *cha, int class_id, int method_id) {
    if (!cha || class_id >= cha->class_count || class_id < 0) return -1;
    if (method_id >= cha->max_methods) return -1;

    KyjitClass *c = &cha->classes[class_id];

    /* Add method to class if not already present */
    for (int i = 0; i < c->method_count; i++) {
        if (c->method_ids[i] == method_id) return 0;
    }

    c->method_ids = realloc(c->method_ids, (c->method_count + 1) * sizeof(int));
    c->method_ids[c->method_count++] = method_id;

    /* Track implementation count for this method */
    if (method_id >= cha->max_methods) {
        fprintf(stderr, "Method ID %d out of range\n", method_id);
        return -1;
    }
    cha->method_to_class[method_id] = class_id;
    cha->method_count[method_id]++;
    return 0;
}

int kyjit_cha_is_monomorphic(KyjitCHA *cha, int root_class_id, int method_id) {
    if (!cha || root_class_id < 0 || root_class_id >= cha->class_count) return -1;
    if (method_id < 0 || method_id >= cha->max_methods) return -1;

    /* Naive: check if exactly one class in the hierarchy implements this method */
    int impl_count = 0;
    int target = -1; (void)target;

    for (int cid = 0; cid < cha->class_count; cid++) {
        KyjitClass *c = &cha->classes[cid];
        for (int i = 0; i < c->method_count; i++) {
            if (c->method_ids[i] == method_id) {
                /* Check if cid is in the hierarchy of root_class_id */
                int ancestor = cid;
                int in_hierarchy = (ancestor == root_class_id);
                while (ancestor != -1 && !in_hierarchy) {
                    ancestor = cha->classes[ancestor].parent_id;
                    if (ancestor == root_class_id) {
                        in_hierarchy = 1;
                        break;
                    }
                }
                if (in_hierarchy) {
                    impl_count++;
                    target = cid;
                }
            }
        }
    }

    if (impl_count == 1) {
        return 1;
    }
    return 0;
}

int kyjit_cha_get_target_class(KyjitCHA *cha, int root_class_id, int method_id) {
    if (!cha) return -1;
    if (kyjit_cha_is_monomorphic(cha, root_class_id, method_id) != 1) return -1;
    return cha->method_to_class[method_id];
}

void kyjit_cha_free(KyjitCHA *cha) {
    if (!cha) return;
    for (int i = 0; i < cha->class_count; i++) {
        free(cha->classes[i].method_ids);
    }
    free(cha->classes);
    free(cha->method_to_class);
    free(cha->method_count);
    free(cha);
}
