// Phase 9 §9.3 fixture: the SYR2K kernel, three nested loops with the k-loop
// reduction-carried. Mirrors the BLAS loops under tests/m5_loops.
function syr2k(alpha, A, B, C, n, ld) {
  for (let j = 0; j < n; j++) {
    for (let i = 0; i < n; i++) {
      let t1 = 0, t2 = 0;
      for (let k = 0; k < n; k++) {
        t1 += alpha * A[k * ld + j] * B[k * ld + i];
        t2 += alpha * B[k * ld + j] * A[k * ld + i];
      }
      C[i * ld + j] += t1 + t2;
    }
  }
  return C;
}
