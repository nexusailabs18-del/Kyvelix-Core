/*
 * kyjit_elf.c — ELF64 executable generator
 *
 * Emits a minimal statically-linked ELF64 binary that, when run,
 * calls the compiled function and exits with its return value.
 *
 * Layout (all little-endian):
 *   0x00   ELF64 header             64 bytes
 *   0x40   Program header            56 bytes
 *   0x78   _start stub               24 bytes
 *   0x90   compiled function code    code_len bytes
 *   ...    (padded to 4096 for the file)
 */

#define _GNU_SOURCE
#include "kyjit_elf.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <errno.h>

/* ---- ELF constants (subset) ---- */
#define EI_NIDENT      16
#define ELFMAG0        0x7F
#define ELFMAG1        'E'
#define ELFMAG2        'L'
#define ELFMAG3        'F'
#define ELFCLASS64     2
#define ELFDATA2LSB    1
#define EV_CURRENT     1
#define ELFOSABI_SYSV  0

#define ET_EXEC        2
#define EM_X86_64      62

#define PT_LOAD        1

#define PF_X           1
#define PF_W           2
#define PF_R           4

/* ---- Layout constants ---- */
#define ELF_BASE       0x400000UL       /* standard Linux text base */
#define ELF_HDR_SIZE   64
#define PHDR_SIZE      56
#define START_STUB     24               /* 8-byte aligned */
#define CODE_OFFSET    (ELF_HDR_SIZE + PHDR_SIZE + START_STUB)  /* 144 */
#define ENTRY_OFFSET   (ELF_HDR_SIZE + PHDR_SIZE)               /* 120 */

/* ---- On-disk structures ---- */

typedef struct __attribute__((packed)) {
    uint8_t  e_ident[EI_NIDENT];
    uint16_t e_type;
    uint16_t e_machine;
    uint32_t e_version;
    uint64_t e_entry;
    uint64_t e_phoff;
    uint64_t e_shoff;
    uint32_t e_flags;
    uint16_t e_ehsize;
    uint16_t e_phentsize;
    uint16_t e_phnum;
    uint16_t e_shentsize;
    uint16_t e_shnum;
    uint16_t e_shstrndx;
} Elf64_Ehdr;

typedef struct __attribute__((packed)) {
    uint32_t p_type;
    uint32_t p_flags;
    uint64_t p_offset;
    uint64_t p_vaddr;
    uint64_t p_paddr;
    uint64_t p_filesz;
    uint64_t p_memsz;
    uint64_t p_align;
} Elf64_Phdr;

/* ---- _start stub ----
 *
 *   call compiled_fn      ; E8 <rel32>
 *   mov rdi, rax          ; 48 89 C7
 *   mov rax, 60           ; 48 C7 C0 3C 00 00 00   (SYS_exit)
 *   syscall               ; 0F 05
 *   hlt                   ; F4 (safety if syscall fails)
 *   nop ...               ; pad to START_STUB bytes
 */
static size_t emit_start_stub(uint8_t *out, uint64_t code_vaddr) {
    uint8_t stub[START_STUB];
    memset(stub, 0x90 /* NOP */, START_STUB);

    /* call compiled_fn */
    stub[0] = 0xE8;
    /* rel32 = code_vaddr - (stub_vaddr + 5) */
    uint64_t stub_vaddr = ELF_BASE + ENTRY_OFFSET;
    int32_t rel32 = (int32_t)(code_vaddr - (stub_vaddr + 5));
    memcpy(&stub[1], &rel32, 4);

    stub[5]  = 0x48; stub[6]  = 0x89; stub[7]  = 0xC7;   /* mov rdi, rax */
    stub[8]  = 0x48; stub[9]  = 0xC7; stub[10] = 0xC0;   /* mov rax, 60 */
    stub[11] = 0x3C; stub[12] = 0x00; stub[13] = 0x00; stub[14] = 0x00;
    stub[15] = 0x0F; stub[16] = 0x05;                    /* syscall */
    stub[17] = 0xF4;                                     /* hlt */

    memcpy(out, stub, START_STUB);
    return START_STUB;
}

int kyx64_write_elf_executable(const uint8_t *code, size_t code_len,
                               const char *out_path, int mode) {
    if (!code || code_len == 0 || !out_path) return -1;

    /* Compute total size — pad to a full page for a clean mmap in the kernel. */
    size_t unpadded = CODE_OFFSET + code_len;
    size_t total    = (unpadded + 0xFFFu) & ~((size_t)0xFFFu);   /* round up to 4K */

    uint8_t *buf = (uint8_t *)calloc(1, total);
    if (!buf) return -1;

    /* --- ELF header --- */
    Elf64_Ehdr eh;
    memset(&eh, 0, sizeof(eh));
    eh.e_ident[0] = ELFMAG0;
    eh.e_ident[1] = ELFMAG1;
    eh.e_ident[2] = ELFMAG2;
    eh.e_ident[3] = ELFMAG3;
    eh.e_ident[4] = ELFCLASS64;
    eh.e_ident[5] = ELFDATA2LSB;
    eh.e_ident[6] = EV_CURRENT;
    eh.e_ident[7] = ELFOSABI_SYSV;

    eh.e_type      = ET_EXEC;
    eh.e_machine   = EM_X86_64;
    eh.e_version   = EV_CURRENT;
    eh.e_entry     = ELF_BASE + ENTRY_OFFSET;
    eh.e_phoff     = ELF_HDR_SIZE;
    eh.e_shoff     = 0;                 /* no section headers */
    eh.e_flags     = 0;
    eh.e_ehsize    = ELF_HDR_SIZE;
    eh.e_phentsize = PHDR_SIZE;
    eh.e_phnum     = 1;
    eh.e_shentsize = 0;
    eh.e_shnum     = 0;
    eh.e_shstrndx  = 0;

    memcpy(buf, &eh, sizeof(eh));

    /* --- Program header --- */
    Elf64_Phdr ph;
    memset(&ph, 0, sizeof(ph));
    ph.p_type   = PT_LOAD;
    ph.p_flags  = PF_R | PF_X;
    ph.p_offset = 0;
    ph.p_vaddr  = ELF_BASE;
    ph.p_paddr  = ELF_BASE;
    ph.p_filesz = total;
    ph.p_memsz  = total;
    ph.p_align  = 0x1000;

    memcpy(buf + ELF_HDR_SIZE, &ph, sizeof(ph));

    /* --- _start stub --- */
    emit_start_stub(buf + ENTRY_OFFSET, ELF_BASE + CODE_OFFSET);

    /* --- Compiled code --- */
    memcpy(buf + CODE_OFFSET, code, code_len);

    /* --- Write to disk --- */
    FILE *f = fopen(out_path, "wb");
    if (!f) { free(buf); return -1; }
    size_t written = fwrite(buf, 1, total, f);
    fclose(f);
    free(buf);

    if (written != total) return -1;

    /* --- chmod --- */
    if (chmod(out_path, (mode_t)mode) != 0) return -1;

    return 0;
}
