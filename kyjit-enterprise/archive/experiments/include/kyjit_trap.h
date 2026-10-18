#ifndef KYJIT_TRAP_H
#define KYJIT_TRAP_H

#include <stdint.h>

typedef enum {
    KYJIT_TRAP_NONE = 0,
    KYJIT_TRAP_SEGV = 1,
    KYJIT_TRAP_ILL  = 2,
    KYJIT_TRAP_FPE  = 3,
    KYJIT_TRAP_BUS  = 4,
} KyjitTrapType;

typedef void (*KyjitTrapHandler)(KyjitTrapType type, void *addr);

int           kyjit_trap_install(KyjitTrapHandler handler);
void          kyjit_trap_uninstall(void);
KyjitTrapType kyjit_trap_last_type(void);
void         *kyjit_trap_last_address(void);

#endif
