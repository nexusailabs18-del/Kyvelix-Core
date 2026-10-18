#ifndef KYJIT_DEOPT_H
#define KYJIT_DEOPT_H

int kyjit_deopt_register(void *optimized, void *fallback);
void kyjit_deopt_patch_to_fallback(int entry_id);
int kyjit_deopt_should_bailout(int entry_id);
void* kyjit_deopt_execute(int entry_id, void *data);

#endif
