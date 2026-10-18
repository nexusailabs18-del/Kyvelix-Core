#ifndef KYJIT_OSR_H
#define KYJIT_OSR_H

#include "kyjit_bytecode.h"

typedef struct KyjitOSR KyjitOSR;

KyjitOSR *kyjit_osr_create(void);
void kyjit_osr_mark_hot(KyjitOSR *osr, int instruction_index);
int kyjit_osr_is_hot(KyjitOSR *osr, int instruction_index);
KyBytecodeBuf *kyjit_osr_compile_hot(KyjitOSR *osr, KyBytecodeBuf *original);
void kyjit_osr_reset(KyjitOSR *osr);
void kyjit_osr_free(KyjitOSR *osr);

#endif
