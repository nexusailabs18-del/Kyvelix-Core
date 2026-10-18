// Phase 9 §9.3 fixture: the triangle kernel, an inner bound that depends on the
// outer IV (a parametric bound, which Phase 5 handles).
function triangle(A, n) {
  let sum = 0;
  for (let i = 0; i < n; i++) {
    for (let j = 0; j <= i; j++) {
      sum += A[i][j];
    }
  }
  return sum;
}
