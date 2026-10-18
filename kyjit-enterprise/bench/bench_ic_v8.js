'use strict';

const RUNS = 5;
const ITERATIONS = parseInt(process.env.KY_BENCH_ITERS || '100000000', 10);

let sink = 0;

function makeObject() { return { x: 42 }; }
function loadX(o) { return o.x; }

// Aggressive warmup — force V8 to reach TurboFan
// V8 needs ~10k calls + 1 feedback cycle to optimize
function warmup() {
  const o = makeObject();
  const warmIterations = Math.max(1, Math.floor(ITERATIONS / 50));
  let acc = 0;
  for (let i = 0; i < warmIterations; ++i) {
    acc += loadX(o);
  }
  sink += acc;
}

function runOnce() {
  const o = makeObject();
  const start = process.hrtime.bigint();
  let acc = 0;
  for (let i = 0; i < ITERATIONS; ++i) {
    acc += loadX(o);
  }
  const end = process.hrtime.bigint();
  sink += acc;
  return Number(end - start) / ITERATIONS;
}

function main() {
  // Warmup twice as long — force full JIT tier
  warmup();
  warmup();

  const results = [];
  for (let i = 0; i < RUNS; ++i) {
    results.push(runOnce());
  }

  results.sort((a, b) => a - b);
  const median = results[2];

  console.log(JSON.stringify({
    benchmark: 'v8_ic_mono',
    hardware: process.env.KY_BENCH_HARDWARE || 'Intel i5-1240P (12C/16T)',
    runtime: process.version,
    iterations: ITERATIONS,
    runs: RUNS,
    ns_per_op: median,
    min_ns: results[0],
    median_ns: median,
    p99_ns: results[4],
    max_ns: results[4],
    sink: sink,
    date: new Date().toISOString().slice(0, 10),
  }, null, 2));
}

main();
