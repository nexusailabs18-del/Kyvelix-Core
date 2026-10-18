/*
 * kyjit_elf.h — ELF64 executable generator
 *
 * Wraps compiled x86-64 machine code in a minimal, standalone ELF64
 * executable that runs on Linux. The generated program:
 *   1. Calls the compiled function (from _start)
 *   2. Exits with the function's return value as the process status
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

/*
 * Write a complete ELF64 executable to out_path.
 *
 * @param code       x86-64 bytes produced by kyx64_compile()
 * @param code_len   length of code in bytes
 * @param out_path   filesystem path to write to (will be overwritten)
 * @param mode       file mode (e.g. 0755 for executable)
 * @return           0 on success, -1 on error
 */
int kyx64_write_elf_executable(const uint8_t *code, size_t code_len,
                               const char *out_path, int mode);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_ELF_H */
