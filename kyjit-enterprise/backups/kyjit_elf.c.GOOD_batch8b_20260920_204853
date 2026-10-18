/*
 * kyjit_elf.c — ELF64 executable generator with optional data section.
 *
 * Layout (all little-endian):
 *   0x0000  ELF header               64 bytes
 *   0x0040  Program header #1         56 bytes  (PT_LOAD RX, code)
 *   0x0078  Program header #2         56 bytes  (PT_LOAD RW, data)
 *   0x00B0  _start stub               24 bytes
 *   0x00C8  compiled function code    code_len bytes
 *   ...     padded to 0x1000
 *   0x1000  data section (if any)     data_len bytes
 *   ...     padded to 0x2000
 */

#define _GNU_SOURCE
#include "kyjit_elf.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

/* ---- ELF constants ---- */
#define EI_NIDENT      16
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

/* ---- Layout ---- */
#define ELF_HDR_SIZE   64
#define PHDR_SIZE      56
#define PHDR_COUNT     2
#define ENTRY_OFFSET   (ELF_HDR_SIZE + PHDR_COUNT * PHDR_SIZE)   /* 176 */
#define CODE_OFFSET    (ENTRY_OFFSET + 24)                       /* 200 */
#define DATA_OFFSET    0x1000                                    /* page 2 */
#define PAGE_SIZE      0x1000

/* ---- Structures ---- */

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

/* _start stub: call compiled_fn; mov rdi,rax; mov rax,60; syscall; hlt */
static void emit_start_stub(uint8_t *out, uint64_t stub_vaddr,
                            uint64_t fn_vaddr) {
    uint8_t stub[24];
    memset(stub, 0x90, sizeof(stub));

    /* call <compiled_fn> */
    stub[0] = 0xE8;
    int32_t rel = (int32_t)(fn_vaddr - (stub_vaddr + 5));
    memcpy(&stub[1], &rel, 4);

    /* mov rdi, rax */
    stub[5] = 0x48; stub[6] = 0x89; stub[7] = 0xC7;

    /* mov rax, 60 (SYS_exit) */
    stub[8] = 0x48; stub[9] = 0xC7; stub[10] = 0xC0;
    stub[11] = 0x3C; stub[12] = 0; stub[13] = 0; stub[14] = 0;

    /* syscall */
    stub[15] = 0x0F; stub[16] = 0x05;

    /* hlt (safety) */
    stub[17] = 0xF4;

    memcpy(out, stub, sizeof(stub));
}

/* Shared implementation */
static int elf_write_impl(const uint8_t *code, size_t code_len,
                          const uint8_t *data, size_t data_len,
                          const char *out_path, int mode) {
    if (!code || code_len == 0 || !out_path) return -1;

    /* Two pages: code page + data page (if data present).
     * If no data, just one page. */
    size_t code_end = CODE_OFFSET + code_len;
    size_t file_size = (code_end + PAGE_SIZE - 1) & ~((size_t)PAGE_SIZE - 1);

    int have_data = (data && data_len > 0);
    if (have_data) {
        file_size = DATA_OFFSET + ((data_len + PAGE_SIZE - 1) &
                                   ~((size_t)PAGE_SIZE - 1));
    }

    uint8_t *buf = (uint8_t *)calloc(1, file_size);
    if (!buf) return -1;

    /* --- ELF header --- */
    Elf64_Ehdr eh;
    memset(&eh, 0, sizeof(eh));
    eh.e_ident[0] = 0x7F;
    eh.e_ident[1] = 'E';
    eh.e_ident[2] = 'L';
    eh.e_ident[3] = 'F';
    eh.e_ident[4] = ELFCLASS64;
    eh.e_ident[5] = ELFDATA2LSB;
    eh.e_ident[6] = EV_CURRENT;
    eh.e_ident[7] = ELFOSABI_SYSV;

    eh.e_type      = ET_EXEC;
    eh.e_machine   = EM_X86_64;
    eh.e_version   = EV_CURRENT;
    eh.e_entry     = KYJIT_ELF_ENTRY_VADDR;
    eh.e_phoff     = ELF_HDR_SIZE;
    eh.e_shoff     = 0;
    eh.e_ehsize    = ELF_HDR_SIZE;
    eh.e_phentsize = PHDR_SIZE;
    eh.e_phnum     = have_data ? 2 : 1;
    eh.e_shentsize = 0;
    eh.e_shnum     = 0;
    eh.e_shstrndx  = 0;

    memcpy(buf, &eh, sizeof(eh));

    /* --- Program header #1: code segment (RX) --- */
    Elf64_Phdr ph1;
    memset(&ph1, 0, sizeof(ph1));
    ph1.p_type   = PT_LOAD;
    ph1.p_flags  = PF_R | PF_X;
    ph1.p_offset = 0;
    ph1.p_vaddr  = KYJIT_ELF_BASE;
    ph1.p_paddr  = KYJIT_ELF_BASE;
    ph1.p_filesz = have_data ? DATA_OFFSET : file_size;
    ph1.p_memsz  = ph1.p_filesz;
    ph1.p_align  = PAGE_SIZE;
    memcpy(buf + ELF_HDR_SIZE, &ph1, sizeof(ph1));

    /* --- Program header #2: data segment (RW), optional --- */
    if (have_data) {
        Elf64_Phdr ph2;
        memset(&ph2, 0, sizeof(ph2));
        ph2.p_type   = PT_LOAD;
        ph2.p_flags  = PF_R | PF_W;
        ph2.p_offset = DATA_OFFSET;
        ph2.p_vaddr  = KYJIT_ELF_DATA_VADDR;
        ph2.p_paddr  = KYJIT_ELF_DATA_VADDR;
        ph2.p_filesz = data_len;
        ph2.p_memsz  = ((data_len + PAGE_SIZE - 1) &
                        ~((size_t)PAGE_SIZE - 1));
        ph2.p_align  = PAGE_SIZE;
        memcpy(buf + ELF_HDR_SIZE + PHDR_SIZE, &ph2, sizeof(ph2));
    }

    /* --- _start stub --- */
    uint64_t stub_vaddr = KYJIT_ELF_ENTRY_VADDR;
    uint64_t fn_vaddr   = KYJIT_ELF_BASE + CODE_OFFSET;
    emit_start_stub(buf + ENTRY_OFFSET, stub_vaddr, fn_vaddr);

    /* --- compiled code --- */
    memcpy(buf + CODE_OFFSET, code, code_len);

    /* --- data --- */
    if (have_data) {
        memcpy(buf + DATA_OFFSET, data, data_len);
    }

    /* --- write to disk --- */
    FILE *f = fopen(out_path, "wb");
    if (!f) { free(buf); return -1; }
    size_t written = fwrite(buf, 1, file_size, f);
    fclose(f);
    free(buf);
    if (written != file_size) return -1;

    if (chmod(out_path, (mode_t)mode) != 0) return -1;

    return 0;
}

int kyx64_write_elf_executable(const uint8_t *code, size_t code_len,
                               const char *out_path, int mode) {
    return elf_write_impl(code, code_len, NULL, 0, out_path, mode);
}

int kyx64_write_elf_executable_ex(const uint8_t *code, size_t code_len,
                                  const uint8_t *data, size_t data_len,
                                  const char *out_path, int mode) {
    return elf_write_impl(code, code_len, data, data_len, out_path, mode);
}
