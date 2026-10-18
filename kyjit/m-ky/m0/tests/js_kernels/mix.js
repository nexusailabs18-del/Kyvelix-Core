// Phase 9 §9.3 fixture: the control forms the kernels do not cover — for-in,
// while with a compound guard, do-while, a strided `for`, and destructuring
// declarations with and without a rest element.
const { a, b = 2 } = obj;
const [x, y, ...rest] = arr;
let total = 0;
for (const key in map) {
  total += map[key] * factor;
}
while (i < hi && j > lo) {
  if (a[i] < b[j]) {
    out.push(a[i]);
    i++;
  } else {
    out.push(b[j]);
    j++;
  }
}
do {
  i--;
} while (i >= 0 && a[i] === 0);
for (let i = 0; i < n; i += 2) {
  a[i] = a[i + 1] + 1;
}
