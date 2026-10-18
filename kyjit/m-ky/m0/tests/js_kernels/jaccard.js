// Phase 9 §9.3 fixture: the Jaccard similarity kernel — a `for-of` over one
// input, a method call in the guard, and a length-dependent final division.
// §9.7 is what turns the `for-of` into an indexed loop.
function jaccard(setA, setB) {
  let intersection = 0;
  for (const item of setA) {
    if (setB.includes(item)) {
      intersection++;
    }
  }
  return intersection / (setA.length + setB.length - intersection);
}
