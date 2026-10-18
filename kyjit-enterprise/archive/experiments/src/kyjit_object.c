#include "kyjit_object.h"
#include <stdlib.h>
#include <string.h>

/* ── Shape ── */

KyjitShape *kyjit_shape_create(uint32_t id) {
    KyjitShape *s = calloc(1, sizeof(*s));
    if (!s) return NULL;
    s->shape_id = id;
    s->prop_count = 0;
    s->props = NULL;
    s->next = NULL;
    return s;
}

int kyjit_shape_add_prop(KyjitShape *s, const char *name, uint32_t type) {
    if (!s || !name) return -1;

    KyjitProperty *new_props = realloc(s->props,
                                       (s->prop_count + 1) * sizeof(KyjitProperty));
    if (!new_props) return -1;

    s->props = new_props;
    s->props[s->prop_count].name = strdup(name);
    s->props[s->prop_count].type = type;

    /* Simple layout: each field aligned to 8 bytes */
    s->props[s->prop_count].offset = s->prop_count * 8;

    s->prop_count++;
    return 0;
}

int kyjit_shape_lookup(KyjitShape *s, const char *name, KyjitProperty *out) {
    if (!s || !name) return -1;

    for (uint32_t i = 0; i < s->prop_count; i++) {
        if (strcmp(s->props[i].name, name) == 0) {
            if (out) *out = s->props[i];
            return 0;
        }
    }
    return -1;
}

/* ── Object ── */

KyjitObject *kyjit_object_create(KyjitShape *shape) {
    if (!shape) return NULL;

    KyjitObject *obj = calloc(1, sizeof(*obj));
    if (!obj) return NULL;

    obj->shape = shape;
    obj->data_size = shape->prop_count * 8;
    if (obj->data_size == 0) obj->data_size = 8;

    obj->data = calloc(1, obj->data_size);
    if (!obj->data) {
        free(obj);
        return NULL;
    }
    return obj;
}

void kyjit_object_free(KyjitObject *obj) {
    if (!obj) return;
    free(obj->data);
    free(obj);
}

/* ── Fast property access ── */

int32_t kyjit_object_get_i32(KyjitObject *obj, uint32_t offset) {
    int32_t v;
    memcpy(&v, obj->data + offset, sizeof(v));
    return v;
}

void kyjit_object_set_i32(KyjitObject *obj, uint32_t offset, int32_t val) {
    memcpy(obj->data + offset, &val, sizeof(val));
}

double kyjit_object_get_f64(KyjitObject *obj, uint32_t offset) {
    double v;
    memcpy(&v, obj->data + offset, sizeof(v));
    return v;
}

void kyjit_object_set_f64(KyjitObject *obj, uint32_t offset, double val) {
    memcpy(obj->data + offset, &val, sizeof(val));
}

void *kyjit_object_get_ptr(KyjitObject *obj, uint32_t offset) {
    void *v;
    memcpy(&v, obj->data + offset, sizeof(v));
    return v;
}

void kyjit_object_set_ptr(KyjitObject *obj, uint32_t offset, void *ptr) {
    memcpy(obj->data + offset, &ptr, sizeof(ptr));
}
