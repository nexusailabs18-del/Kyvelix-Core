/*
 * kyjit_elf.h — ELF64 executable generator
 *
 * Wraps compiled x86-64 machine code in a minimal, standalone ELF64
 * executable that runs on Linux. The generated program:
 *   1. Calls the compiled function (from _start)
 *   2. Exits with the function's return value as the process status
 *
 * Optional data section is placed at a FIXED virtual address
 * (KYJIT_ELF_DATA_VADDR). IR that references this address can be
 * compiled before the data is written.
 *
 * No libc, no dynamic linker. Static.
 */

#ifndef KYJIT_ELF_H
#define KYJIT_ELF_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Fixed virtual addresses in every generated executable. */
#define KYJIT_ELF_BASE          0x400000ULL   /* ELF image base */
#define KYJIT_ELF_CODE_VADDR    0x400000ULL   /* code segment starts here */
#define KYJIT_ELF_ENTRY_VADDR   0x4000B0ULL   /* _start stub */
#define KYJIT_ELF_DATA_VADDR    0x401000ULL   /* data segment (page 2) */

/*
 * Write a complete ELF64 executable with no data section.
 *
 * @param code       x86-64 bytes produced by kyx64_compile()
 * @param code_len   length of code in bytes
 * @param out_path   filesystem path to write to
 * @param mode       file mode (e.g. 0755)
 * @return           0 on success, -1 on error
 */
int kyx64_write_elf_executable(const uint8_t *code, size_t code_len,
                               const char *out_path, int mode);

/*
 * Write a complete ELF64 executable with an optional data section.
 * The data is placed at KYJIT_ELF_DATA_VADDR. IR should pass that
 * address as a constant when it needs a pointer into the data.
 *
 * @param code       x86-64 bytes produced by kyx64_compile()
 * @param code_len   length of code in bytes
 * @param data       optional read-only data (NULL if none)
 * @param data_len   length of data in bytes (0 if none)
 * @param out_path   filesystem path to write to
 * @param mode       file mode (e.g. 0755)
 * @return           0 on success, -1 on error
 */
int kyx64_write_elf_executable_ex(const uint8_t *code, size_t code_len,
                                  const uint8_t *data, size_t data_len,
                                  uint64_t data_vaddr,
                                  const char *out_path, int mode);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_ELF_H */
