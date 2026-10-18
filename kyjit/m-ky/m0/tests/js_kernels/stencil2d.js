// Phase 9 §9.3 fixture: the 2-D five-point stencil, the Phase 6c/7 shape where
// one statement reads four neighbours of the same array. All offsets are affine
// in the IVs, which is what the dependence tests key on.
function stencil(u, f, n, h) {
  for (let i = 1; i < n; i++) {
    for (let j = 1; j < n; j++) {
      u[i][j] = (u[i - 1][j] + u[i + 1][j] + u[i][j - 1] + u[i][j + 1] + h * h * f[i][j]) / 4;
    }
  }
  return u;
}
