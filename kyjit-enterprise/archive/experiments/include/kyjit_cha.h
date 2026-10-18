#ifndef KYJIT_CHA_H
#define KYJIT_CHA_H

#include <stdint.h>

/* Class metadata */
typedef struct {
    int     class_id;
    int     parent_id;         /* -1 for root */
    int     method_count;
    int    *method_ids;        /* IDs of virtual methods declared here */
    int     is_abstract;
} KyjitClass;

typedef struct KyjitCHA KyjitCHA;

/* Create CHA */
KyjitCHA *kyjit_cha_create(int max_classes);

/* Register a class */
int kyjit_cha_add_class(KyjitCHA *cha, int class_id, int parent_id, int is_abstract);

/* Register a method implementation in a class */
int kyjit_cha_add_method(KyjitCHA *cha, int class_id, int method_id);

/* Check if a virtual call is monomorphic (only one target in hierarchy).
 * Returns:
 *   1 → single implementation (can devirtualize)
 *   0 → multiple implementations (polymorphic)
 *  -1 → error / unknown method
 */
int kyjit_cha_is_monomorphic(KyjitCHA *cha, int root_class_id, int method_id);

/* Get the single target class for a monomorphic call */
int kyjit_cha_get_target_class(KyjitCHA *cha, int root_class_id, int method_id);

/* Free */
void kyjit_cha_free(KyjitCHA *cha);

#endif
